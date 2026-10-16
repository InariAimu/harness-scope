use crate::{harness::files::hash, model::*, query, store::Store, sync::validate_record};
use anyhow::{Context, Result};
use chrono::TimeZone;
use rusqlite::{backup::Backup, params, types::ValueRef, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationInput {
    pub path: String,
    pub source_id: String,
    pub cutover_date: String,
    pub timezone: String,
    pub expected_digest: Option<String>,
}
pub struct Preview {
    pub records: Vec<UsageRecord>,
    pub value: Value,
    pub cutoff: i64,
    pub digest: String,
    pub harnesses: Vec<String>,
}
fn rows(conn: &Connection, table: &str) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(&format!("SELECT * FROM {table}"))?;
    let names = stmt
        .column_names()
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut m = serde_json::Map::new();
        for (i, name) in names.iter().enumerate() {
            let value = match row.get_ref(i)? {
                ValueRef::Null => Value::Null,
                ValueRef::Integer(n) => json!(n),
                ValueRef::Real(n) => json!(n.to_string()),
                ValueRef::Text(b) => json!(std::str::from_utf8(b)?),
                ValueRef::Blob(_) => Value::Null,
            };
            m.insert(name.clone(), value);
        }
        out.push(Value::Object(m));
    }
    Ok(out)
}
fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
fn number(v: &Value, key: &str) -> u64 {
    v[key].as_u64().unwrap_or(0)
}
fn fold(s: &str) -> &str {
    if s == "claude-desktop" {
        "claude"
    } else {
        s
    }
}
fn is_duplicate(v: &Value, proxies: &[&Value]) -> bool {
    let ds = text(v, "data_source");
    if ![
        "session_log",
        "codex_session",
        "gemini_session",
        "opencode_session",
    ]
    .contains(&ds)
    {
        return false;
    }
    proxies.iter().any(|p| {
        fold(text(p, "app_type")) == fold(text(v, "app_type"))
            && number(p, "input_tokens") == number(v, "input_tokens")
            && number(p, "output_tokens") == number(v, "output_tokens")
            && number(p, "cache_read_tokens") == number(v, "cache_read_tokens")
            && (number(p, "cache_creation_tokens") == number(v, "cache_creation_tokens")
                || (number(v, "cache_creation_tokens") == 0
                    && ["codex_session", "gemini_session", "opencode_session"].contains(&ds))
                || (number(p, "cache_creation_tokens") == 0
                    && number(p, "input_token_semantics") == 0
                    && ds == "codex_session"))
            && (p["created_at"].as_i64().unwrap_or(0) - v["created_at"].as_i64().unwrap_or(0)).abs()
                <= 600
            && (text(p, "model").eq_ignore_ascii_case(text(v, "model"))
                || text(p, "model").eq_ignore_ascii_case("unknown")
                || text(v, "model").eq_ignore_ascii_case("unknown"))
    })
}
pub fn preview(store: &Store, input: &MigrationInput) -> Result<Preview> {
    anyhow::ensure!(
        !input.source_id.is_empty()
            && input.source_id.len() <= 100
            && input
                .source_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        "legacy source ID must contain letters, digits, - or _"
    );
    let timezone: chrono_tz::Tz = input.timezone.parse().context("Invalid IANA timezone")?;
    let date = chrono::NaiveDate::parse_from_str(&input.cutover_date, "%Y-%m-%d")?;
    let cutoff = timezone
        .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
        .earliest()
        .context("Invalid cutover midnight")?
        .timestamp();
    let source =
        Connection::open_with_flags(Path::new(&input.path), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    source.busy_timeout(std::time::Duration::from_secs(3))?;
    let mut snapshot = Connection::open_in_memory()?;
    Backup::new(&source, &mut snapshot)?.run_to_completion(
        100,
        std::time::Duration::from_millis(5),
        None,
    )?;
    let version: i64 = snapshot.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    anyhow::ensure!(
        version == 20,
        "Only CC Switch schema v20 is supported (found v{version})"
    );
    let details = rows(&snapshot, "proxy_request_logs")?;
    let days = rows(&snapshot, "usage_daily_rollups")?;
    let proxies = details
        .iter()
        .filter(|v| {
            let ds = text(v, "data_source");
            (ds == "proxy" || ds.is_empty()) && (200..300).contains(&number(v, "status_code"))
        })
        .collect::<Vec<_>>();
    let mut records = Vec::new();
    let own = store.device_id()?;
    let mut deduplicated = 0;
    for (grain, values) in [("event", &details), ("legacy_day", &days)] {
        for v in values {
            let at = if grain == "event" {
                v["created_at"].as_i64().context("Missing created_at")?
            } else {
                let date = chrono::NaiveDate::parse_from_str(text(v, "date"), "%Y-%m-%d")?;
                timezone
                    .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
                    .earliest()
                    .context("Invalid legacy day")?
                    .timestamp()
            };
            if at >= cutoff {
                continue;
            }
            if grain == "event" && is_duplicate(v, &proxies) {
                deduplicated += 1;
                continue;
            }
            let harness = fold(text(v, "app_type")).to_owned();
            let read = number(v, "cache_read_tokens");
            let write = number(v, "cache_creation_tokens");
            let raw_input = number(v, "input_tokens");
            let inclusive = ["codex", "gemini", "grokbuild"].contains(&harness.as_str());
            let semantic = number(v, "input_token_semantics");
            let input_tokens = if inclusive && semantic != 2 {
                raw_input
                    .saturating_sub(read)
                    .saturating_sub(if semantic == 1 { write } else { 0 })
            } else {
                raw_input
            };
            let key = if grain == "event" {
                text(v, "request_id").into()
            } else {
                hash(
                    serde_json::to_string(&json!([
                        v["date"],
                        v["app_type"],
                        v["provider_id"],
                        v["model"],
                        v["request_model"],
                        v["pricing_model"]
                    ]))?
                    .as_bytes(),
                )
            };
            let mut cost = Cost {
                amount_usd: Some(text(v, "total_cost_usd").to_owned()),
                method: "legacy".into(),
                ..Default::default()
            };
            if cost.amount_usd.as_deref() == Some("") {
                cost.amount_usd = None;
                cost.method = "unknown".into()
            }
            let r = UsageRecord {
                device_id: own.clone(),
                id: format!("legacy:{}:{grain}:{key}", input.source_id),
                harness,
                parser_version: "ccswitch-v20".into(),
                session_id: v["session_id"].as_str().map(str::to_owned),
                native_event_id: if grain == "event" { Some(key) } else { None },
                occurred_at: at,
                model: v["pricing_model"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(text(v, "model"))
                    .into(),
                grain: grain.into(),
                count: if grain == "event" {
                    1
                } else {
                    number(v, "request_count")
                },
                tokens: Tokens {
                    input: input_tokens,
                    output: number(v, "output_tokens"),
                    cache_read: read,
                    cache_write: write,
                    ..Default::default()
                },
                cost,
                record_source: if grain == "event" {
                    "ccswitch_detail"
                } else {
                    "ccswitch_rollup"
                }
                .into(),
                revision: 1,
                deleted: false,
                status_code: if grain == "event" && text(v, "data_source") == "proxy" {
                    v["status_code"].as_u64().map(|n| n as u16)
                } else {
                    None
                },
                legacy_date: if grain == "legacy_day" {
                    Some(text(v, "date").into())
                } else {
                    None
                },
                legacy_timezone: if grain == "legacy_day" {
                    Some(input.timezone.clone())
                } else {
                    None
                },
            };
            validate_record(&r)?;
            records.push(r);
        }
    }
    records.sort_by(|a, b| a.id.cmp(&b.id));
    let harnesses = records
        .iter()
        .map(|r| r.harness.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let digest=hash(serde_json::to_string(&json!({"records":records,"cutoff":cutoff,"timezone":input.timezone,"sourceId":input.source_id}))?.as_bytes());
    Ok(Preview {
        value: json!({"digest":digest,"schemaVersion":version,"recordCount":records.len(),"detailCount":records.iter().filter(|r|r.grain=="event").count(),"dailyCount":records.iter().filter(|r|r.grain=="legacy_day").count(),"deduplicated":deduplicated,"summary":query::summary(&records),"harnesses":harnesses,"cutover":cutoff,"timezone":input.timezone}),
        records,
        cutoff,
        digest,
        harnesses,
    })
}
pub fn commit(store: &Store, input: &MigrationInput) -> Result<Value> {
    let p = preview(store, input)?;
    anyhow::ensure!(
        input.expected_digest.as_deref() == Some(&p.digest),
        "Snapshot changed or preview missing; preview again"
    );
    store.transaction(||{
        let manifests:Vec<Value>=store.meta("migration_manifests")?.unwrap_or_default();
        for existing in &manifests{
            if existing["sourceId"]==input.source_id{anyhow::ensure!(existing["digest"]==p.digest,"This source was already migrated from a different snapshot");return Ok(p.value.clone())}
            let previous:Vec<String>=serde_json::from_value(existing["harnesses"].clone())?;
            anyhow::ensure!(!previous.iter().any(|h|p.harnesses.contains(h)),"An overlapping harness was already migrated on this device");
        }
        for mut old in store.local_records()?.into_iter().filter(|r|r.record_source=="session"&&r.occurred_at<p.cutoff&&p.harnesses.contains(&r.harness)){
            old.deleted=true;store.upsert(old,"migration-superseded")?;
        }
        for r in &p.records{store.upsert(r.clone(),&format!("legacy:{}",input.source_id))?;}
        let mut manifests=manifests;manifests.push(json!({"sourceId":input.source_id,"digest":p.digest,"cutoff":p.cutoff,"timezone":input.timezone,"harnesses":p.harnesses}));
        store.set_meta("migration_manifests",&manifests)?;
        store.conn.execute("INSERT INTO migration_sources(id,cutover,timezone,manifest) VALUES(?1,?2,?3,?4)",params![input.source_id,p.cutoff,input.timezone,p.value.to_string()])?;
        store.conn.execute("DELETE FROM file_state",[])?;Ok(p.value)
    })
}
