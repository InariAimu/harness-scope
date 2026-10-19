use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    #[serde(default)]
    pub cache_write1h: u64,
    #[serde(default)]
    pub reasoning: u64,
}
impl Tokens {
    pub fn total(&self) -> u64 {
        self.input
            .saturating_add(self.output)
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Price {
    pub model_id: String,
    pub display_name: String,
    pub input_cost_per_million: String,
    pub output_cost_per_million: String,
    pub cache_read_cost_per_million: String,
    pub cache_creation_cost_per_million: String,
    pub source: String,
    pub source_key: Option<String>,
    pub fetched_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cost {
    pub amount_usd: Option<String>,
    pub method: String,
    pub price_source: String,
    pub pricing_model: Option<String>,
    pub priced_at: i64,
    pub rule_version: String,
    pub snapshot: Option<Price>,
}
impl Default for Cost {
    fn default() -> Self {
        Self {
            amount_usd: None,
            method: "unknown".into(),
            price_source: "unknown".into(),
            pricing_model: None,
            priced_at: 0,
            rule_version: "ccswitch-e8880e6-v1".into(),
            snapshot: None,
        }
    }
}

/// This DTO is the wire allowlist. Paths, transcripts and credentials cannot serialize here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UsageRecord {
    pub device_id: String,
    pub id: String,
    pub harness: String,
    pub parser_version: String,
    pub session_id: Option<String>,
    pub native_event_id: Option<String>,
    pub occurred_at: i64,
    pub model: String,
    pub grain: String,
    pub count: u64,
    pub tokens: Tokens,
    pub cost: Cost,
    pub record_source: String,
    pub revision: u64,
    pub deleted: bool,
    pub status_code: Option<u16>,
    pub legacy_date: Option<String>,
    pub legacy_timezone: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedUsage {
    pub id: String,
    pub session_id: String,
    pub native_event_id: String,
    pub occurred_at: i64,
    pub model: String,
    pub tokens: Tokens,
    pub reported_cost: Option<String>,
    pub timing: Option<UsageTiming>,
}

/// Local log estimates, deliberately separate from the v1 usage sync allowlist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageTiming {
    pub duration_ms: i64,
    pub first_token_ms: Option<i64>,
    pub duration_source: String,
    pub first_token_source: Option<String>,
}
impl UsageTiming {
    pub fn estimate(
        start: i64,
        end: i64,
        first: Option<i64>,
        source: &str,
        first_source: &str,
    ) -> Option<Self> {
        let duration_ms = end.checked_sub(start)?;
        // Same plausibility window as CC Switch's Codex log estimates.
        if start <= 0 || !(100..=3_600_000).contains(&duration_ms) {
            return None;
        }
        let first_token_ms = first
            .and_then(|t| t.checked_sub(start))
            .filter(|t| (0..=duration_ms).contains(t));
        Some(Self {
            duration_ms,
            first_token_ms,
            duration_source: source.into(),
            first_token_source: first_token_ms.map(|_| first_source.into()),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: String,
    pub harness: String,
    pub root: String,
    pub enabled: bool,
}

/// Device-local display preferences. Amounts in facts and sync remain USD.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UsageDisplaySettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exchange_rates: Option<std::collections::BTreeMap<String, f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sidebar_today_items: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_log_columns: Option<std::collections::BTreeMap<String, bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trend_chart_type: Option<String>,
}
impl UsageDisplaySettings {
    pub fn apply(&mut self, patch: Self) -> anyhow::Result<()> {
        const CURRENCIES: &[&str] = &["USD", "CNY", "EUR", "JPY", "TWD", "TRY"];
        if let Some(currency) = patch.currency {
            anyhow::ensure!(
                CURRENCIES.contains(&currency.as_str()),
                "Unsupported display currency"
            );
            self.currency = Some(currency);
        }
        if let Some(rates) = patch.exchange_rates {
            for (currency, rate) in &rates {
                anyhow::ensure!(
                    CURRENCIES.contains(&currency.as_str())
                        && currency != "USD"
                        && rate.is_finite()
                        && *rate > 0.,
                    "Invalid display exchange rate"
                );
            }
            self.exchange_rates
                .get_or_insert_with(Default::default)
                .extend(rates);
        }
        if let Some(items) = patch.sidebar_today_items {
            let unique: std::collections::HashSet<_> = items.iter().collect();
            anyhow::ensure!(
                items.len() <= 2
                    && unique.len() == items.len()
                    && items
                        .iter()
                        .all(|s| ["cost", "tokens", "requests", "hitRate"].contains(&s.as_str())),
                "Choose up to two distinct sidebar metrics"
            );
            self.sidebar_today_items = Some(items);
        }
        if let Some(columns) = patch.request_log_columns {
            anyhow::ensure!(
                columns.len() <= 64 && columns.get("time") != Some(&false),
                "The request time column must stay visible"
            );
            self.request_log_columns = Some(columns);
        }
        if let Some(chart_type) = patch.trend_chart_type {
            anyhow::ensure!(
                ["bar", "area"].contains(&chart_type.as_str()),
                "Unsupported trend chart type"
            );
            self.trend_chart_type = Some(chart_type);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub device_name: String,
    pub sources: Vec<Source>,
    pub server_url: String,
    pub server_token: String,
    pub scan_interval_seconds: u64,
    pub theme: String,
    pub language: String,
    pub timezone: String,
    #[serde(default = "default_models_sync")]
    pub models_dev_sync: serde_json::Value,
}
fn default_models_sync() -> serde_json::Value {
    serde_json::json!({"autoSyncEnabled":true,"includeCommonModels":true,"selectedModelKeys":[],"excludedCommonModelKeys":[],"lastSyncAt":null,"lastSyncError":null})
}
impl Default for Config {
    fn default() -> Self {
        Self {
            device_name: std::env::var("COMPUTERNAME")
                .or_else(|_| std::env::var("HOSTNAME"))
                .unwrap_or_else(|_| "My device".into()),
            sources: Vec::new(),
            server_url: String::new(),
            server_token: String::new(),
            scan_interval_seconds: 60,
            theme: "system".into(),
            language: "zh".into(),
            timezone: "Asia/Shanghai".into(),
            models_dev_sync: default_models_sync(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub provider_id: String,
    pub session_id: String,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub project_dir: Option<String>,
    pub created_at: Option<i64>,
    pub last_active_at: Option<i64>,
    pub source_path: Option<String>,
    pub resume_command: Option<String>,
    #[serde(default)]
    pub can_delete: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub imported: u64,
    pub updated: u64,
    pub skipped: u64,
    pub files_scanned: u64,
    pub suspected_duplicates: u64,
    pub deferred_files: u64,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PushRequest {
    pub protocol_version: u32,
    pub device_id: String,
    pub device_name: String,
    pub records: Vec<UsageRecord>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ack {
    pub id: String,
    pub revision: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushResponse {
    pub server_instance_id: String,
    pub accepted: Vec<Ack>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub seq: i64,
    pub record: UsageRecord,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangesResponse {
    pub server_instance_id: String,
    pub changes: Vec<Change>,
    pub next_cursor: i64,
    pub has_more: bool,
    pub devices: Vec<serde_json::Value>,
}
