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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: String,
    pub harness: String,
    pub root: String,
    pub enabled: bool,
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
