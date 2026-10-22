//! Device-local account settings and balances, with linked usage across synced devices.
use crate::{account_credentials::Vault, query_store::Selection, store::Store, Engine};
use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashSet},
    io::Read,
    str::FromStr,
    time::Duration,
};

const META: &str = "provider_accounts_v1";
mod analysis;
pub(crate) mod codex;
mod custom;
mod go_usage;
mod history;
mod polling;
pub(crate) use polling::Polling;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct RefreshPolicy {
    pub foreground_seconds: u64,
    pub background_seconds: u64,
}
impl Default for RefreshPolicy {
    fn default() -> Self {
        Self {
            foreground_seconds: 60,
            background_seconds: 300,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Link {
    pub harness: String,
    pub provider: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Subscription {
    #[serde(default)]
    pub plan_name: String,
    pub monthly_fee: Option<String>,
    #[serde(default = "usd")]
    pub currency: String,
    pub period_start: Option<String>,
    pub renewal_date: Option<String>,
}
fn usd() -> String {
    "USD".into()
}
impl Default for Subscription {
    fn default() -> Self {
        Self {
            plan_name: String::new(),
            monthly_fee: None,
            currency: usd(),
            period_start: None,
            renewal_date: None,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Account {
    pub id: String,
    pub provider: String,
    pub name: String,
    #[serde(default = "default_show_in_sidebar")]
    pub show_in_sidebar: bool,
    pub links: Vec<Link>,
    pub subscription: Subscription,
    #[serde(default)]
    pub refresh: RefreshPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex: Option<codex::Settings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<custom::Settings>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Save {
    id: Option<String>,
    provider: String,
    name: String,
    #[serde(default)]
    show_in_sidebar: Option<bool>,
    #[serde(default)]
    links: Vec<Link>,
    #[serde(default)]
    subscription: Subscription,
    api_key: Option<String>,
    #[serde(default)]
    clear_api_key: bool,
    #[serde(default)]
    refresh: RefreshPolicy,
    #[serde(default)]
    codex: Option<codex::Settings>,
    #[serde(default)]
    custom: Option<custom::Settings>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    account: Account,
    revision: String,
    snapshot: Option<Value>,
    last_attempt_at: Option<i64>,
    error: Option<String>,
    #[serde(default)]
    request_id: Option<String>,
}
fn load(store: &Store) -> Result<Vec<Stored>> {
    Ok(store.meta(META)?.unwrap_or_default())
}
fn default_show_in_sidebar() -> bool {
    true
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WeeklyUsageWindow {
    start_date: i64,
    resets_at: i64,
    bucket: Option<String>,
}
fn weekly_usage_window(item: &Stored, now: i64) -> Option<WeeklyUsageWindow> {
    let data = item.snapshot.as_ref()?.get("data")?;
    let (windows, bucket) = match item.account.provider.as_str() {
        "openai-codex" if data["kind"] == "codexQuota" => {
            let buckets = data["buckets"].as_array()?;
            // Use the standard Codex bucket; an alternative is safe only when
            // it is the sole bucket, so unrelated model quotas are not guessed.
            let bucket = buckets
                .iter()
                .find(|b| b["id"] == "codex")
                .or_else(|| (buckets.len() == 1).then(|| &buckets[0]))?;
            (
                bucket["windows"].as_array()?,
                bucket["id"].as_str().map(str::to_owned),
            )
        }
        "opencode-go" if data["kind"] == "quota" => (data["windows"].as_array()?, None),
        _ => return None,
    };
    let mut weekly = windows.iter().filter(|window| {
        if item.account.provider == "openai-codex" {
            window["durationMinutes"] == 10080
        } else {
            window["id"] == "weekly"
        }
    });
    let window = weekly.next()?;
    if weekly.next().is_some() {
        return None;
    }
    let resets_at = window["resetsAt"].as_i64()?;
    let start_date = resets_at.checked_sub(7 * 86400)?;
    if resets_at <= now || start_date > now {
        return None;
    }
    Some(WeeklyUsageWindow {
        start_date,
        resets_at,
        bucket,
    })
}
fn view(engine: &Engine, item: &Stored, vault: &Vault) -> Value {
    json!({"account":item.account,"keyConfigured":vault.exists(&item.account.id),"canRefresh":can_refresh(&item.account,vault),"snapshot":item.snapshot,"lastAttemptAt":item.last_attempt_at,"error":item.error,"refreshing":engine.account_polling.is_refreshing(&item.account.id),"weeklyUsageWindow":weekly_usage_window(item, Utc::now().timestamp())})
}
fn can_refresh(account: &Account, vault: &Vault) -> bool {
    if let Some(settings) = &account.custom {
        settings.enabled && (!settings.script.contains("{{apiKey}}") || vault.exists(&account.id))
    } else {
        account.provider == "openai-codex" || vault.exists(&account.id)
    }
}
fn id(args: &Value) -> Result<&str> {
    let id = args["id"].as_str().context("Missing provider account ID")?;
    uuid::Uuid::parse_str(id).context("Invalid provider account ID")?;
    Ok(id)
}
fn validate(input: &Save) -> Result<()> {
    for interval in [
        input.refresh.foreground_seconds,
        input.refresh.background_seconds,
    ] {
        anyhow::ensure!(
            interval == 0 || (15..=86400).contains(&interval),
            "Refresh interval must be 0 or 15–86400 seconds"
        );
    }
    anyhow::ensure!(
        ["deepseek", "opencode-go", "openai-codex"].contains(&input.provider.as_str())
            || custom::is_provider(&input.provider),
        "Unsupported provider"
    );
    if custom::is_provider(&input.provider) {
        custom::validate(
            input
                .custom
                .as_ref()
                .context("Missing custom query settings")?,
        )?;
    } else {
        anyhow::ensure!(
            input.custom.is_none(),
            "Custom settings require a custom provider"
        );
    }
    if input.provider == "openai-codex" {
        codex::validate(
            input
                .codex
                .as_ref()
                .context("Choose a Codex estimation plan")?,
        )?;
        anyhow::ensure!(
            input.api_key.as_deref().is_none_or(|v| v.trim().is_empty()) && !input.clear_api_key,
            "Codex subscription uses local login, not an API key"
        );
        anyhow::ensure!(
            input
                .links
                .iter()
                .all(|v| v.harness == "codex" && v.provider == "openai"),
            "Link Codex / openai routes for subscription estimates"
        );
    } else {
        anyhow::ensure!(
            input.codex.is_none(),
            "Codex settings require a Codex subscription account"
        );
    }
    anyhow::ensure!(
        !input.name.trim().is_empty() && input.name.chars().count() <= 80,
        "Account name must be 1–80 characters"
    );
    anyhow::ensure!(input.links.len() <= 128, "Too many linked routes");
    let mut links = HashSet::new();
    for link in &input.links {
        anyhow::ensure!(
            crate::providers::valid(&link.provider)
                && !link.harness.is_empty()
                && link.harness.len() <= 64,
            "Invalid linked route"
        );
        anyhow::ensure!(
            links.insert((&link.harness, &link.provider)),
            "Duplicate linked route"
        );
    }
    let s = &input.subscription;
    anyhow::ensure!(s.plan_name.chars().count() <= 80, "Plan name is too long");
    anyhow::ensure!(
        s.currency.len() == 3 && s.currency.bytes().all(|c| c.is_ascii_uppercase()),
        "Use a three-letter currency code"
    );
    if let Some(amount) = &s.monthly_fee {
        let number = Decimal::from_str(amount).context("Invalid monthly fee")?;
        anyhow::ensure!(
            number >= Decimal::ZERO && number <= Decimal::from(1_000_000),
            "Monthly fee is out of range"
        );
    }
    for date in [&s.period_start, &s.renewal_date].into_iter().flatten() {
        anyhow::ensure!(date.len() == 10, "Use YYYY-MM-DD dates");
        NaiveDate::parse_from_str(date, "%Y-%m-%d").context("Invalid subscription date")?;
    }
    if let (Some(start), Some(end)) = (&s.period_start, &s.renewal_date) {
        anyhow::ensure!(start < end, "Renewal date must be after the period start");
    }
    if let Some(key) = input.api_key.as_deref().filter(|k| !k.trim().is_empty()) {
        anyhow::ensure!(
            !input.clear_api_key,
            "Cannot replace and clear the API key together"
        );
        anyhow::ensure!(
            key.trim().len() <= 4096 && key.trim().bytes().all(|c| c.is_ascii_graphic()),
            "API key contains invalid characters"
        );
    }
    Ok(())
}
pub(crate) fn execute(engine: &Engine, vault: &Vault, name: &str, args: Value) -> Result<Value> {
    match name {
        "provider_accounts" => Ok(json!(load(&*engine.read_lock()?)?
            .iter()
            .map(|a| view(engine, a, vault))
            .collect::<Vec<_>>())),
        "provider_account_history" => history::query(&*engine.read_lock()?, args),
        "provider_account_estimate" => analysis::estimate(engine, args),
        "provider_account_test_query" => custom::test_query(engine, vault, args),
        "provider_session_sources" => custom::session_sources(engine, args),
        "provider_account_routes" => {
            let store = engine.read_lock()?;
            let selected = Selection::new(&store, &json!({"scope":"all"}), i64::MIN, i64::MAX)?;
            let mut statement = store.conn.prepare_cached(&format!("SELECT u.harness,u.provider,COUNT(*) FROM usage_index u WHERE {} AND u.provider IS NOT NULL GROUP BY u.harness,u.provider ORDER BY u.harness,u.provider", selected.sql))?;
            let rows = statement.query_map(rusqlite::params_from_iter(selected.params.iter()), |r| Ok(json!({"harness":r.get::<_,String>(0)?,"provider":r.get::<_,String>(1)?,"records":r.get::<_,i64>(2)?})))?.collect::<Result<Vec<_>,_>>()?;
            Ok(json!(rows))
        }
        "provider_account_save" => save(engine, vault, serde_json::from_value(args)?),
        "provider_account_delete" => {
            let id = id(&args)?;
            let store = engine.lock()?;
            let mut accounts = load(&store)?;
            anyhow::ensure!(
                accounts.iter().any(|a| a.account.id == id),
                "Provider account no longer exists"
            );
            accounts.retain(|a| a.account.id != id);
            let old = vault.raw(id)?;
            vault.restore(id, None)?;
            if let Err(error) = store.transaction(|| {
                codex::calibration::clear(&store, id)?;
                store.set_meta(META, &accounts)
            }) {
                vault.restore(id, old.as_deref())?;
                return Err(error);
            }
            Ok(json!(true))
        }
        "provider_account_refresh" => {
            polling::refresh_managed(engine, vault, id(&args)?, |provider, key| {
                fetch(engine, provider, key)
            })
        }
        "provider_account_activity" => polling::activity(engine, &args),
        "go_usage_estimate" => go_usage::estimate(engine, &args),
        "codex_subscription_estimate" => codex::estimate(engine, &args),
        "go_forecast" => go_usage::forecast(engine, &args),
        "go_simulate" => go_usage::simulate(engine, &args),
        "provider_account_usage" => linked_usage(
            engine,
            id(&args)?,
            args["range"].as_str().unwrap_or("month"),
        ),
        _ => anyhow::bail!("Unknown provider account command"),
    }
}
fn save(engine: &Engine, vault: &Vault, mut input: Save) -> Result<Value> {
    input.name = input.name.trim().into();
    validate(&input)?;
    let store = engine.lock()?;
    let mut items = load(&store)?;
    anyhow::ensure!(
        items.len() < 100 || input.id.is_some(),
        "At most 100 provider accounts are supported"
    );
    let new = input.id.is_none();
    let account_id = input
        .id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if input.provider == "custom" {
        anyhow::ensure!(new, "Choose the saved custom provider ID");
        input.provider = format!("custom:{account_id}");
    }
    if custom::is_provider(&input.provider) {
        anyhow::ensure!(
            input.provider == format!("custom:{account_id}"),
            "Invalid custom provider ID"
        );
    }
    uuid::Uuid::parse_str(&account_id).context("Invalid provider account ID")?;
    let position = items.iter().position(|a| a.account.id == account_id);
    anyhow::ensure!(
        new || position.is_some(),
        "Provider account no longer exists"
    );
    for other in items.iter().filter(|a| a.account.id != account_id) {
        anyhow::ensure!(
            !input
                .links
                .iter()
                .any(|link| other.account.links.contains(link)),
            "A selected route is already linked to another account"
        );
    }
    let provider_changed = position.is_some_and(|i| items[i].account.provider != input.provider);
    let connection_changed = position.is_some_and(|i| {
        items[i]
            .account
            .codex
            .as_ref()
            .map(|v| (&v.binary_path, &v.home_path))
            != input.codex.as_ref().map(|v| (&v.binary_path, &v.home_path))
            || input.provider == "openai-codex" && items[i].account.links != input.links
            || items[i].account.custom != input.custom
    });
    let new_key = input
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    anyhow::ensure!(
        !provider_changed || new_key.is_some() || input.clear_api_key,
        "Changing provider requires a new key or clearing the saved key"
    );
    let mut item = position
        .map(|i| items[i].clone())
        .unwrap_or_else(|| Stored {
            account: Account {
                id: account_id.clone(),
                provider: input.provider.clone(),
                name: String::new(),
                show_in_sidebar: true,
                links: vec![],
                subscription: Subscription::default(),
                refresh: RefreshPolicy::default(),
                codex: None,
                custom: None,
            },
            revision: String::new(),
            snapshot: None,
            last_attempt_at: None,
            error: None,
            request_id: None,
        });
    item.account = Account {
        id: account_id.clone(),
        provider: input.provider,
        name: input.name,
        show_in_sidebar: input
            .show_in_sidebar
            .unwrap_or(item.account.show_in_sidebar),
        links: input.links,
        subscription: input.subscription,
        refresh: input.refresh,
        codex: input.codex,
        custom: input.custom,
    };
    item.revision = uuid::Uuid::new_v4().to_string();
    if new_key.is_some() || input.clear_api_key || provider_changed || connection_changed {
        item.snapshot = None;
        item.error = None;
        item.last_attempt_at = None;
    }
    let old_key = vault.raw(&account_id)?;
    if let Some(key) = new_key {
        vault.write(&account_id, key)?;
    } else if input.clear_api_key {
        vault.restore(&account_id, None)?;
    }
    if let Some(i) = position {
        items[i] = item.clone();
    } else {
        items.push(item.clone());
    }
    if let Err(error) = store.transaction(|| {
        if connection_changed || provider_changed {
            codex::calibration::clear(&store, &account_id)?;
        }
        store.set_meta(META, &items)
    }) {
        vault.restore(&account_id, old_key.as_deref())?;
        return Err(error);
    }
    Ok(view(engine, &item, vault))
}
fn refresh_with(
    engine: &Engine,
    vault: &Vault,
    id: &str,
    fetch: impl FnOnce(&str, &str) -> Result<Value>,
) -> Result<Value> {
    let (captured, key, request) = {
        let store = engine.lock()?;
        let mut items = load(&store)?;
        let item = items
            .iter_mut()
            .find(|a| a.account.id == id)
            .context("Provider account no longer exists")?;
        let key = if let Some(settings) = &item.account.custom {
            if !settings.enabled {
                anyhow::bail!("Custom usage query is disabled");
            }
            let key = if settings.script.contains("{{apiKey}}") {
                vault.read(id)
            } else {
                Ok(String::new())
            };
            key.and_then(|key| Ok(serde_json::to_string(&(settings, key))?))
        } else if item.account.provider == "openai-codex" {
            serde_json::to_string(&item.account.codex).map_err(anyhow::Error::from)
        } else {
            vault.read(id)
        };
        let request = uuid::Uuid::new_v4().to_string();
        item.request_id = Some(request.clone());
        item.last_attempt_at = Some(Utc::now().timestamp());
        let captured = item.clone();
        store.set_meta(META, &items)?;
        (captured, key, request)
    };
    // Both DB mutexes are released during DNS, HTTP, response decoding and validation.
    // Credential failures must also record an attempt, so a moved/corrupt vault cannot
    // spin the automatic scheduler or silently hide the error from the account page.
    let response = key.and_then(|key| fetch(&captured.account.provider, &key));
    let observed_at = Utc::now();
    let store = engine.lock()?;
    let mut items = load(&store)?;
    let item = items
        .iter_mut()
        .find(|a| a.account.id == id)
        .context("Provider account no longer exists")?;
    if item.revision == captured.revision && item.request_id.as_deref() == Some(&request) {
        let previous_snapshot = item.snapshot.clone();
        let succeeded = response.is_ok();
        match response {
            Ok(data) => {
                item.snapshot = Some(json!({"fetchedAt":observed_at.timestamp(),"data":data}));
                item.error = None;
            }
            Err(error) => item.error = Some(error.to_string()),
        }
        let result = view(engine, item, vault);
        let observed = succeeded.then(|| item.clone());
        store.transaction(|| {
            if let Some(observed) = observed.as_ref() {
                history::record(&store, observed, observed_at, &store.config()?.timezone)?;
                if observed.account.provider == "openai-codex" {
                    codex::calibration::record(
                        &store,
                        id,
                        previous_snapshot.as_ref(),
                        observed.snapshot.as_ref().unwrap(),
                    )?;
                }
            }
            store.set_meta(META, &items)
        })?;
        Ok(result)
    } else {
        Ok(view(engine, item, vault))
    }
}
fn fetch(engine: &Engine, provider: &str, key: &str) -> Result<Value> {
    let network = crate::network::load(&*engine.read_lock()?)?;
    if custom::is_provider(provider) {
        let (settings, secret): (custom::Settings, String) = serde_json::from_str(key)?;
        return custom::request(&settings, &secret, &network);
    }
    if provider == "openai-codex" {
        return codex::fetch(&serde_json::from_str(key)?, &network);
    }
    let url = match provider {
        "deepseek" => "https://api.deepseek.com/user/balance",
        "opencode-go" => "https://opencode.ai/inference/go/v1/usage",
        _ => anyhow::bail!("Unsupported provider"),
    };
    request_with_network(provider, key, url, &network)
}
#[cfg(test)]
fn request(provider: &str, key: &str, url: &str) -> Result<Value> {
    request_with_network(provider, key, url, &crate::network::Settings::default())
}
fn request_with_network(
    provider: &str,
    key: &str,
    url: &str,
    network: &crate::network::Settings,
) -> Result<Value> {
    let client = network.client()?;
    let response = client
        .get(url)
        .bearer_auth(key)
        .header("Accept", "application/json")
        .send()
        .map_err(|_| anyhow::anyhow!("Provider request failed or timed out. Please retry."))?;
    match response.status().as_u16() {
        200 => (),
        401 => anyhow::bail!("API key is invalid or expired (401)"),
        403 => {
            anyhow::bail!("The key has no permission or an active subscription is required (403)")
        }
        429 => anyhow::bail!("Provider rate limit reached; retry later (429)"),
        _ => anyhow::bail!("Provider returned HTTP {}", response.status().as_u16()),
    }
    let mut bytes = Vec::new();
    response
        .take(131073)
        .read_to_end(&mut bytes)
        .map_err(|_| anyhow::anyhow!("Cannot read provider response"))?;
    anyhow::ensure!(bytes.len() <= 131072, "Provider response is too large");
    let value = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("Provider returned invalid JSON"))?;
    parse_response(provider, value)
}
pub fn parse_response(provider: &str, raw: Value) -> Result<Value> {
    // Project an allowlist. No upstream diagnostic bodies, account IDs or echoed keys are persisted.
    if provider == "deepseek" {
        let available = raw["is_available"]
            .as_bool()
            .context("Missing balance availability")?;
        let rows = raw["balance_infos"]
            .as_array()
            .filter(|v| !v.is_empty() && v.len() <= 10)
            .context("Missing balance details")?;
        let mut balances = Vec::new();
        let mut currencies = HashSet::new();
        for row in rows {
            let currency = row["currency"]
                .as_str()
                .context("Missing balance currency")?;
            anyhow::ensure!(
                ["CNY", "USD"].contains(&currency) && currencies.insert(currency),
                "Invalid balance currency"
            );
            let money = |field: &str| -> Result<String> {
                let text = row[field].as_str().context("Invalid balance amount")?;
                let value = Decimal::from_str(text).context("Invalid balance amount")?;
                Ok(value.normalize().to_string())
            };
            balances.push(json!({"currency":currency,"total":money("total_balance")?,"granted":money("granted_balance")?,"toppedUp":money("topped_up_balance")?}));
        }
        Ok(json!({"kind":"balance","available":available,"balances":balances}))
    } else if provider == "opencode-go" {
        let mut windows = Vec::new();
        for name in ["rolling", "weekly", "monthly"] {
            let row = &raw["usage"][name];
            let percent = row["percent"]
                .as_f64()
                .filter(|v| v.is_finite() && *v >= 0.0 && *v <= 10000.0)
                .context("Invalid subscription usage percentage")?;
            let reset = row["resetsAt"]
                .as_str()
                .and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok())
                .context("Invalid subscription reset time")?;
            let status = match row["status"].as_str() {
                Some("ok") => "ok",
                Some("rate-limited") => "rate-limited",
                Some(_) => "unknown",
                None => anyhow::bail!("Missing subscription status"),
            };
            windows.push(json!({"id":name,"usedPercent":percent,"resetsAt":reset.timestamp(),"status":status}));
        }
        Ok(json!({"kind":"quota","windows":windows}))
    } else {
        anyhow::bail!("Unsupported provider")
    }
}
fn midnight(timezone: chrono_tz::Tz, day: NaiveDate) -> Result<i64> {
    use chrono::Duration;
    let start = day.and_hms_opt(0, 0, 0).unwrap();
    for minute in 0..1440 {
        if let Some(value) = timezone
            .from_local_datetime(&(start + Duration::minutes(minute)))
            .earliest()
        {
            return Ok(value.timestamp());
        }
    }
    anyhow::bail!("Cannot resolve local date")
}
// Linked routes are assumed to share this subscription on every synced device.
// Selection excludes the server's mirror of this device, as well as tombstones.
fn linked_selection(store: &Store, account: &Account, start: i64, end: i64) -> Result<Selection> {
    let mut selected = Selection::new(store, &json!({"scope":"all"}), start, end)?;
    if account.links.is_empty() {
        selected.sql.push_str(" AND 0");
    } else {
        selected.sql.push_str(" AND (");
        for (i, link) in account.links.iter().enumerate() {
            if i > 0 {
                selected.sql.push_str(" OR ");
            }
            selected.sql.push_str("(u.harness=? AND u.provider=?)");
            selected
                .params
                .extend([link.harness.clone().into(), link.provider.clone().into()]);
        }
        selected.sql.push(')');
    }
    Ok(selected)
}
fn linked_usage(engine: &Engine, id: &str, range: &str) -> Result<Value> {
    let store = engine.read_lock()?;
    store.transaction(|| {
        let item = load(&store)?.into_iter().find(|a| a.account.id == id).context("Provider account no longer exists")?;
        let timezone: chrono_tz::Tz = store.config()?.timezone.parse()?;
        let now = Utc::now(); let today = now.with_timezone(&timezone).date_naive();
        let month = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();
        let weekly_window = if range == "weekly" {
            Some(weekly_usage_window(&item, now.timestamp()).context("Weekly quota window unavailable; refresh official quota")?)
        } else {
            None
        };
        let start = if let Some(window) = &weekly_window {
            window.start_date
        } else {
            let start_day = match range {
                "today" => today,
                "month" => month,
                "subscription" => item.account.subscription.period_start.as_deref().map(|v| NaiveDate::parse_from_str(v, "%Y-%m-%d")).transpose()?.unwrap_or(month),
                _ => anyhow::bail!("Invalid usage range"),
            };
            midnight(timezone, start_day)?
        };
        let mut end = now.timestamp();
        if range == "subscription" {
            if let Some(date) = &item.account.subscription.renewal_date {
                end = end.min(midnight(timezone, NaiveDate::parse_from_str(date, "%Y-%m-%d")?)? - 1);
            }
        }
        let selected = linked_selection(&store, &item.account, start, end)?;
        let rows = selected.rows(&store, false)?.into_iter().filter(|r| {
            if r.grain != "legacy_day" { return true; }
            let bounds = || -> Result<(i64,i64)> {
                let tz: chrono_tz::Tz = r.legacy_timezone.as_deref().context("Missing legacy timezone")?.parse()?;
                let date = NaiveDate::parse_from_str(r.legacy_date.as_deref().context("Missing legacy date")?, "%Y-%m-%d")?;
                Ok((midnight(tz,date)?,midnight(tz,date.succ_opt().context("Invalid legacy day")?)?))
            };
            bounds().is_ok_and(|(a,b)| start <= a && end >= b-1)
        }).collect::<Vec<_>>();
        let summary = crate::query::summary_rows(&rows);
        let mut models: BTreeMap<String,Vec<_>> = BTreeMap::new();
        for row in rows { models.entry(row.model.clone()).or_default().push(row); }
        let mut models = models.into_iter().map(|(model,rows)| json!({"model":model,"summary":crate::query::summary_rows(&rows)})).collect::<Vec<_>>();
        models.sort_by_key(|v| std::cmp::Reverse(v["summary"]["totalRequests"].as_u64().unwrap_or(0)));
        models.truncate(8);
        Ok(json!({"linked":!item.account.links.is_empty(),"startDate":start,"endDate":end,"timezone":timezone.name(),"summary":summary,"models":models,"weeklyUsageWindow":weekly_window}))
    })
}

#[cfg(test)]
mod tests;

pub(crate) fn start_background(engine: &std::sync::Arc<Engine>) {
    polling::start_background(engine);
}
