//! Sequential-device handoff: immutable quota observations and explicit subscription bindings.
//! No credentials, executable paths, custom scripts or local refresh preferences cross the wire.
use super::*;
use rusqlite::{params, OptionalExtension};

pub const CAPABILITY: &str = "subscription_history_v1";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionInfo {
    pub id: String,
    pub provider: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Observation {
    pub id: String,
    pub subscription_id: String,
    pub device_id: String,
    pub account_id: String,
    pub account_revision: String,
    pub account_name: String,
    pub provider: String,
    pub observed_at_ms: i64,
    pub time_zone: String,
    pub data: Value,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Push {
    pub device_id: String,
    pub server_instance_id: String,
    pub subscriptions: Vec<SubscriptionInfo>,
    pub observations: Vec<Observation>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ack {
    pub server_instance_id: String,
    pub accepted: Vec<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Changes {
    pub server_instance_id: String,
    pub subscriptions: Vec<SubscriptionInfo>,
    pub observations: Vec<(i64, Observation)>,
    pub next_cursor: i64,
    pub has_more: bool,
}

pub(super) fn family(provider: &str) -> &str {
    if custom::is_provider(provider) {
        "custom"
    } else {
        provider
    }
}
fn uuid(value: &str) -> Result<()> {
    uuid::Uuid::parse_str(value).context("Invalid subscription identity")?;
    Ok(())
}
fn fields(value: &Value, allowed: &[&str]) -> Result<()> {
    let object = value.as_object().context("Expected quota object")?;
    anyhow::ensure!(
        object.keys().all(|k| allowed.contains(&k.as_str())),
        "Unsupported quota field"
    );
    Ok(())
}
fn items<'a>(value: &'a Value, key: &str, max: usize) -> Result<&'a Vec<Value>> {
    let rows = value[key].as_array().context("Missing quota items")?;
    anyhow::ensure!(rows.len() <= max, "Too many quota items");
    Ok(rows)
}
fn quota_windows(value: &Value) -> Result<()> {
    for w in items(value, "windows", 20)? {
        fields(
            w,
            &["id", "durationMinutes", "usedPercent", "status", "resetsAt"],
        )?;
    }
    Ok(())
}
fn validate(o: &Observation) -> Result<()> {
    for id in [&o.id, &o.subscription_id, &o.account_id] {
        uuid(id)?;
    }
    anyhow::ensure!(
        !o.device_id.is_empty()
            && o.device_id.len() <= 128
            && !o.account_revision.is_empty()
            && o.account_revision.len() <= 128
            && o.account_name.len() <= 320,
        "Invalid observation identity"
    );
    anyhow::ensure!(
        o.observed_at_ms >= 0
            && chrono::DateTime::from_timestamp_millis(o.observed_at_ms).is_some(),
        "Invalid observation time"
    );
    o.time_zone
        .parse::<chrono_tz::Tz>()
        .context("Invalid observation timezone")?;
    anyhow::ensure!(
        serde_json::to_vec(&o.data)?.len() <= 65536,
        "Quota observation too large"
    );
    match (o.provider.as_str(), o.data["kind"].as_str()) {
        ("deepseek", Some("balance")) => {
            fields(&o.data, &["kind", "available", "balances"])?;
            for b in items(&o.data, "balances", 20)? {
                fields(b, &["currency", "total", "granted", "toppedUp"])?;
            }
        }
        ("opencode-go", Some("quota")) => {
            fields(&o.data, &["kind", "windows"])?;
            quota_windows(&o.data)?;
        }
        ("openai-codex", Some("codexQuota")) => {
            fields(
                &o.data,
                &["kind", "identity", "identitySource", "planType", "buckets"],
            )?;
            for b in items(&o.data, "buckets", 32)? {
                fields(b, &["id", "name", "planType", "windows", "credits"])?;
                quota_windows(b)?;
                if !b["credits"].is_null() {
                    fields(&b["credits"], &["hasCredits", "unlimited", "balance"])?;
                }
            }
        }
        ("custom", Some("customUsage")) => {
            fields(&o.data, &["kind", "items"])?;
            for i in items(&o.data, "items", 20)? {
                fields(
                    i,
                    &[
                        "id",
                        "planName",
                        "unit",
                        "remaining",
                        "used",
                        "total",
                        "usedPercent",
                        "resetsAt",
                        "durationMinutes",
                        "windowType",
                    ],
                )?;
            }
        }
        _ => anyhow::bail!("Quota provider and data kind differ"),
    }
    // Values at leaf positions cannot hide arbitrary nested response objects.
    fn leaves(v: &Value, key: &str) -> bool {
        match v {
            Value::Object(m) if ["", "credits"].contains(&key) => {
                m.iter().all(|(k, v)| leaves(v, k))
            }
            Value::Array(a) if ["balances", "windows", "buckets", "items"].contains(&key) => {
                a.iter().all(|v| leaves(v, ""))
            }
            Value::Object(_) | Value::Array(_) => false,
            Value::String(s) => s.len() <= 1024,
            _ => true,
        }
    }
    anyhow::ensure!(leaves(&o.data, ""), "Invalid nested quota data");
    Ok(())
}

pub fn list(store: &Store) -> Result<Vec<SubscriptionInfo>> {
    let mut stmt = store
        .conn
        .prepare("SELECT id,provider,name FROM shared_subscriptions ORDER BY name,id")?;
    let rows = stmt
        .query_map([], |r| {
            Ok(SubscriptionInfo {
                id: r.get(0)?,
                provider: r.get(1)?,
                name: r.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}
fn insert_subscription(store: &Store, sub: &SubscriptionInfo, pending: bool) -> Result<()> {
    uuid(&sub.id)?;
    anyhow::ensure!(
        ["deepseek", "opencode-go", "openai-codex", "custom"].contains(&sub.provider.as_str())
            && !sub.name.trim().is_empty()
            && sub.name.len() <= 320,
        "Invalid shared subscription"
    );
    if let Some(old) = list(store)?.into_iter().find(|v| v.id == sub.id) {
        anyhow::ensure!(old == *sub, "Shared subscription identity conflict");
    } else {
        store.conn.execute(
            "INSERT INTO shared_subscriptions(id,provider,name,pending) VALUES(?1,?2,?3,?4)",
            params![sub.id, sub.provider, sub.name, pending],
        )?;
    }
    Ok(())
}

/// Binding is explicit and permanent for this local account. Use a new account for another subscription.
pub(super) fn configure(
    store: &Store,
    item: &Account,
    old: Option<&str>,
    requested: Option<&str>,
) -> Result<Option<String>> {
    if let Some(old) = old {
        anyhow::ensure!(requested.is_none_or(|v| v == old), "This account is already linked; create another local account for a different subscription");
        let sub = list(store)?
            .into_iter()
            .find(|v| v.id == old)
            .context("Shared subscription missing")?;
        anyhow::ensure!(
            sub.provider == family(&item.provider),
            "Shared subscription provider cannot change"
        );
        return Ok(Some(old.into()));
    }
    let Some(requested) = requested.filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    let id = if requested == "new" {
        let sub = SubscriptionInfo {
            id: uuid::Uuid::new_v4().to_string(),
            provider: family(&item.provider).into(),
            name: item.name.clone(),
        };
        insert_subscription(store, &sub, true)?;
        sub.id
    } else {
        let sub = list(store)?
            .into_iter()
            .find(|v| v.id == requested)
            .context("Sync first to download this shared subscription")?;
        anyhow::ensure!(
            sub.provider == family(&item.provider),
            "Shared subscription provider differs"
        );
        sub.id
    };
    anyhow::ensure!(
        !load(store)?
            .iter()
            .any(|i| i.account.id != item.id
                && i.account.shared_subscription_id.as_deref() == Some(&id)),
        "Shared subscription already linked to another local account"
    );
    Ok(Some(id))
}

pub(super) fn enqueue(store: &Store, item: &Account, payload: &Value) -> Result<()> {
    let Some(subscription) = &item.shared_subscription_id else {
        return Ok(());
    };
    let text = |key: &str| -> Result<String> {
        Ok(payload[key]
            .as_str()
            .with_context(|| format!("Missing observation {key}"))?
            .into())
    };
    let o = Observation {
        id: text("refreshId")?,
        subscription_id: subscription.clone(),
        device_id: store.device_id()?,
        account_id: item.id.clone(),
        account_revision: text("accountRevision")?,
        account_name: text("accountName")?,
        provider: family(&item.provider).into(),
        observed_at_ms: payload["observedAtMs"]
            .as_i64()
            .context("Invalid observation time")?,
        time_zone: text("timeZone")?,
        data: payload["data"].clone(),
    };
    validate(&o)?;
    cache(store, &o, true)?;
    Ok(())
}
fn cache(store: &Store, o: &Observation, pending: bool) -> Result<bool> {
    let old: Option<String> = store
        .conn
        .query_row(
            "SELECT payload FROM subscription_observations WHERE id=?",
            [&o.id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(old) = old {
        anyhow::ensure!(
            serde_json::from_str::<Observation>(&old)? == *o,
            "Observation identity conflict"
        );
        return Ok(false);
    }
    store.conn.execute("INSERT INTO subscription_observations(id,subscription_id,device_id,payload,pending) VALUES(?1,?2,?3,?4,?5)", params![o.id,o.subscription_id,o.device_id,serde_json::to_string(o)?,pending])?;
    Ok(true)
}
pub(super) fn prepare(store: &Store) -> Result<()> {
    store.transaction(|| {
        for item in load(store)?.into_iter().filter(|i| i.account.shared_subscription_id.is_some()) {
            let mut stmt = store.conn.prepare("SELECT payload FROM provider_account_observations o WHERE account_id=? AND NOT EXISTS(SELECT 1 FROM subscription_observations s WHERE s.id=o.refresh_id)")?;
            let rows = stmt.query_map([&item.account.id], |r| r.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;
            for p in rows { enqueue(store, &item.account, &serde_json::from_str(&p)?)?; }
        }
        Ok(())
    })
}
pub fn pending_count(store: &Store) -> Result<u64> {
    Ok(store.conn.query_row("SELECT (SELECT COUNT(*) FROM subscription_observations WHERE pending=1)+(SELECT COUNT(*) FROM shared_subscriptions WHERE pending=1)",[],|r|r.get(0))?)
}
pub(super) fn history_version(store: &Store) -> Result<i64> {
    Ok(store.conn.query_row(
        "SELECT COALESCE(MAX(seq),0) FROM provider_account_observations",
        [],
        |r| r.get(0),
    )?)
}
pub(super) fn subscription_for(store: &Store, id: &str) -> Result<Option<String>> {
    Ok(load(store)?
        .into_iter()
        .find(|i| i.account.id == id)
        .and_then(|i| i.account.shared_subscription_id))
}

pub fn push(store: &Store, request: Push) -> Result<Ack> {
    let instance: String = store
        .meta("server_instance_id")?
        .context("Missing server identity")?;
    anyhow::ensure!(
        request.server_instance_id == instance,
        "Server changed during subscription upload"
    );
    anyhow::ensure!(
        request.observations.len() <= 200 && request.subscriptions.len() <= 200,
        "Subscription batch too large"
    );
    store.transaction(|| {
        for sub in &request.subscriptions {
            insert_subscription(store, sub, false)?;
        }
        let subs = list(store)?;
        let mut accepted = Vec::new();
        for o in request.observations {
            validate(&o)?;
            anyhow::ensure!(
                o.device_id == request.device_id,
                "Observation device differs from batch"
            );
            anyhow::ensure!(
                subs.iter()
                    .any(|s| s.id == o.subscription_id && s.provider == o.provider),
                "Unknown observation subscription"
            );
            let old: Option<String> = store
                .conn
                .query_row(
                    "SELECT payload FROM server_subscription_observations WHERE id=?",
                    [&o.id],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(old) = old {
                anyhow::ensure!(
                    serde_json::from_str::<Observation>(&old)? == o,
                    "Observation identity conflict"
                );
            } else {
                store.conn.execute(
                    "INSERT INTO server_subscription_observations(id,payload) VALUES(?1,?2)",
                    params![o.id, serde_json::to_string(&o)?],
                )?;
            }
            accepted.push(o.id);
        }
        Ok(Ack {
            server_instance_id: instance,
            accepted,
        })
    })
}
pub fn changes(store: &Store, after: i64, limit: usize) -> Result<Changes> {
    anyhow::ensure!(
        after >= 0 && (1..=500).contains(&limit),
        "Invalid subscription cursor"
    );
    let mut stmt = store.conn.prepare(
        "SELECT seq,payload FROM server_subscription_observations WHERE seq>? ORDER BY seq LIMIT ?",
    )?;
    let rows = stmt
        .query_map(params![after, limit + 1], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let has_more = rows.len() > limit;
    let observations = rows
        .into_iter()
        .take(limit)
        .map(|(seq, p)| Ok((seq, serde_json::from_str(&p)?)))
        .collect::<Result<Vec<_>>>()?;
    let next_cursor = observations.last().map(|(s, _)| *s).unwrap_or(after);
    Ok(Changes {
        server_instance_id: store.meta("server_instance_id")?.unwrap(),
        subscriptions: list(store)?,
        observations,
        next_cursor,
        has_more,
    })
}
pub fn apply(store: &Store, response: &Changes) -> Result<()> {
    store.transaction(|| {
        for sub in &response.subscriptions { insert_subscription(store, sub, false)?; }
        let subs = list(store)?;
        for (_, o) in &response.observations {
            validate(o)?;
            anyhow::ensure!(subs.iter().any(|s| s.id == o.subscription_id && s.provider == o.provider), "Unknown observation subscription");
            let payload = history::shared_payload(o)?;
            // An echo must not overwrite the original local sample or create a second sample.
            let existing: Option<String> = store.conn.query_row("SELECT payload FROM provider_account_observations WHERE refresh_id=?",[&o.id],|r|r.get(0)).optional()?;
            if let Some(p) = existing {
                let p: Value = serde_json::from_str(&p)?;
                anyhow::ensure!(p["accountId"] == o.account_id && p["observedAtMs"] == o.observed_at_ms && p["data"] == o.data, "Observation collides with local history");
            }
            cache(store, o, false)?;
            store.conn.execute("INSERT INTO provider_account_observations(refresh_id,account_id,provider,observed_at_ms,payload) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(refresh_id) DO NOTHING",params![o.id,o.account_id,o.provider,o.observed_at_ms,serde_json::to_string(&payload)?])?;
        }
        store.set_meta("subscription_download_cursor", &response.next_cursor)?;
        store.set_meta("subscription_server_instance", &response.server_instance_id)
    })
}

pub(crate) fn sync(
    engine: &Engine,
    client: &reqwest::blocking::Client,
    base: &str,
    token: &str,
    instance: &str,
) -> Result<Value> {
    {
        let s = engine.lock()?;
        prepare(&s)?;
        if s.meta::<String>("subscription_server_instance")?.as_deref() != Some(instance) {
            s.transaction(|| {
                s.set_meta("subscription_download_cursor", &0)?;
                s.set_meta("subscription_server_instance", &instance)?;
                s.conn.execute(
                    "UPDATE subscription_observations SET pending=1 WHERE device_id=?",
                    [s.device_id()?],
                )?;
                s.conn
                    .execute("UPDATE shared_subscriptions SET pending=1", [])?;
                Ok(())
            })?;
        }
    }
    let mut uploaded = 0;
    loop {
        let request = {
            let s = engine.lock()?;
            // 50 maximum-size observations plus descriptors stay below the decoded 5 MiB limit.
            let mut stmt = s.conn.prepare("SELECT payload FROM subscription_observations WHERE pending=1 ORDER BY id LIMIT 50")?;
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            if rows.is_empty() && pending_count(&s)? == 0 {
                break;
            }
            let observations = rows
                .into_iter()
                .map(|p| serde_json::from_str::<Observation>(&p).map_err(Into::into))
                .collect::<Result<Vec<_>>>()?;
            let mut needed: HashSet<String> = observations
                .iter()
                .map(|o| o.subscription_id.clone())
                .collect();
            let mut stmt = s.conn.prepare(
                "SELECT id FROM shared_subscriptions WHERE pending=1 ORDER BY id LIMIT ?",
            )?;
            let pending = stmt
                .query_map([200 - needed.len()], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            needed.extend(pending);
            Push {
                device_id: s.device_id()?,
                server_instance_id: instance.into(),
                subscriptions: list(&s)?
                    .into_iter()
                    .filter(|s| needed.contains(&s.id))
                    .collect(),
                observations,
            }
        };
        let expected: HashSet<_> = request.observations.iter().map(|o| o.id.clone()).collect();
        let response: Ack = client
            .post(format!("{base}/v1/subscriptions/push"))
            .bearer_auth(token)
            .header("Content-Type", "application/json")
            .header("Content-Encoding", "gzip")
            .body(crate::sync::gzip_json(&request)?)
            .send()?
            .error_for_status()?
            .json()?;
        anyhow::ensure!(
            response.server_instance_id == instance
                && response.accepted.len() == expected.len()
                && response.accepted.iter().cloned().collect::<HashSet<_>>() == expected,
            "Invalid subscription acknowledgment"
        );
        let s = engine.lock()?;
        s.transaction(|| {
            for id in &response.accepted {
                s.conn.execute(
                    "UPDATE subscription_observations SET pending=0 WHERE id=?",
                    [id],
                )?;
            }
            for sub in &request.subscriptions {
                s.conn.execute(
                    "UPDATE shared_subscriptions SET pending=0 WHERE id=?",
                    [&sub.id],
                )?;
            }
            Ok(())
        })?;
        uploaded += response.accepted.len();
    }
    let mut downloaded = 0;
    loop {
        let cursor = engine
            .lock()?
            .meta::<i64>("subscription_download_cursor")?
            .unwrap_or(0);
        let response: Changes = client
            .get(format!("{base}/v1/subscriptions/changes"))
            .bearer_auth(token)
            .query(&[("after", cursor), ("limit", 500)])
            .send()?
            .error_for_status()?
            .json()?;
        anyhow::ensure!(
            response.server_instance_id == instance
                && response.next_cursor >= cursor
                && (!response.has_more || response.next_cursor > cursor),
            "Subscription server or cursor changed; retry"
        );
        let mut previous = cursor;
        let mut ids = HashSet::new();
        anyhow::ensure!(
            response.observations.len() <= 500,
            "Subscription page too large"
        );
        for (seq, observation) in &response.observations {
            anyhow::ensure!(
                *seq > previous && ids.insert(&observation.id),
                "Invalid subscription change order"
            );
            previous = *seq;
        }
        anyhow::ensure!(
            previous == response.next_cursor,
            "Subscription cursor skips observations"
        );
        downloaded += response.observations.len();
        apply(&*engine.lock()?, &response)?;
        if !response.has_more {
            break;
        }
    }
    Ok(json!({"uploaded":uploaded,"downloaded":downloaded}))
}

#[cfg(test)]
mod tests;
