//! One backend scheduler owns automatic network requests, including when no page is mounted.
use super::*;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};

const LEASE_SECONDS: u64 = 45;
const MAX_IN_FLIGHT: usize = 4;

#[derive(Default)]
pub(crate) struct Polling {
    // Per mounted page lease: an old page's cleanup must not deactivate a new page.
    presence: Mutex<HashMap<String, (String, Instant)>>,
    in_flight: Mutex<HashSet<String>>,
}
impl Polling {
    pub fn is_refreshing(&self, id: &str) -> bool {
        self.in_flight.lock().is_ok_and(|v| v.contains(id))
    }
    fn begin(self: &Arc<Self>, id: &str, scheduled: bool) -> Result<Option<Guard>> {
        let mut running = self
            .in_flight
            .lock()
            .map_err(|_| anyhow::anyhow!("Provider scheduler lock poisoned"))?;
        if running.contains(id) || (scheduled && running.len() >= MAX_IN_FLIGHT) {
            return Ok(None);
        }
        running.insert(id.into());
        Ok(Some(Guard {
            polling: self.clone(),
            id: id.into(),
        }))
    }
    fn active(&self, now: Instant) -> HashSet<String> {
        let Ok(mut leases) = self.presence.lock() else {
            return HashSet::new();
        };
        leases.retain(|_, (_, at)| {
            now.saturating_duration_since(*at) < Duration::from_secs(LEASE_SECONDS)
        });
        leases.values().map(|(id, _)| id.clone()).collect()
    }
}
struct Guard {
    polling: Arc<Polling>,
    id: String,
}
impl Drop for Guard {
    fn drop(&mut self) {
        if let Ok(mut running) = self.polling.in_flight.lock() {
            running.remove(&self.id);
        }
    }
}
pub(super) fn activity(engine: &Engine, args: &Value) -> Result<Value> {
    let account_id = id(args)?;
    let lease = args["lease"]
        .as_str()
        .context("Missing provider page lease")?;
    uuid::Uuid::parse_str(lease).context("Invalid provider page lease")?;
    let active = args["active"]
        .as_bool()
        .context("Missing provider page activity")?;
    let mut leases = engine
        .account_polling
        .presence
        .lock()
        .map_err(|_| anyhow::anyhow!("Provider scheduler lock poisoned"))?;
    if active {
        anyhow::ensure!(
            leases.len() < 256 || leases.contains_key(lease),
            "Too many provider page leases"
        );
        leases.insert(lease.into(), (account_id.into(), Instant::now()));
    } else {
        leases.remove(lease);
    }
    Ok(json!(true))
}
pub(super) fn due_at(
    engine: &Engine,
    vault: &Vault,
    now: i64,
    clock: Instant,
) -> Result<Vec<String>> {
    let active = engine.account_polling.active(clock);
    let accounts = load(&*engine.read_lock()?)?;
    let mut due = accounts
        .into_iter()
        .filter(|item| {
            let foreground = active.contains(&item.account.id);
            let seconds = if foreground {
                item.account.refresh.foreground_seconds
            } else {
                item.account.refresh.background_seconds
            };
            if seconds == 0
                || !can_refresh(&item.account, vault)
                || engine.account_polling.is_refreshing(&item.account.id)
            {
                return false;
            }
            // Do not turn transient provider errors into a tight retry loop.
            let seconds = if item.error.is_some() {
                seconds.max(60)
            } else {
                seconds
            };
            item.last_attempt_at
                .is_none_or(|last| now.saturating_sub(last) >= seconds as i64)
        })
        .collect::<Vec<_>>();
    due.sort_by_key(|item| {
        (
            !active.contains(&item.account.id),
            item.last_attempt_at.unwrap_or(0),
            item.account.id.clone(),
        )
    });
    Ok(due.into_iter().map(|item| item.account.id).collect())
}
pub(super) fn refresh_managed(
    engine: &Engine,
    vault: &Vault,
    id: &str,
    fetch: impl FnOnce(&str, &str) -> Result<Value>,
) -> Result<Value> {
    let Some(guard) = engine.account_polling.begin(id, false)? else {
        let store = engine.read_lock()?;
        let item = load(&store)?
            .into_iter()
            .find(|a| a.account.id == id)
            .context("Provider account no longer exists")?;
        return Ok(view(engine, &item, vault));
    };
    let mut result = refresh_with(engine, vault, id, fetch);
    drop(guard);
    if let Ok(value) = &mut result {
        value["refreshing"] = json!(false);
    }
    result
}
pub(super) fn tick(
    engine: &Arc<Engine>,
    fetch: fn(&Engine, &str, &str) -> Result<Value>,
) -> Result<usize> {
    let mut started = 0;
    for id in due_at(
        engine,
        &engine.account_vault,
        Utc::now().timestamp(),
        Instant::now(),
    )? {
        let Some(guard) = engine.account_polling.begin(&id, true)? else {
            continue;
        };
        let engine = engine.clone();
        std::thread::spawn(move || {
            let _guard = guard;
            let _ = refresh_with(&engine, &engine.account_vault, &id, |provider, key| {
                fetch(&engine, provider, key)
            });
        });
        started += 1;
    }
    Ok(started)
}
pub(super) fn start_background(engine: &Arc<Engine>) {
    let weak = Arc::downgrade(engine);
    std::thread::spawn(move || loop {
        let Some(engine) = weak.upgrade() else { break };
        let _ = tick(&engine, fetch);
        drop(engine);
        std::thread::sleep(Duration::from_secs(2));
    });
}
