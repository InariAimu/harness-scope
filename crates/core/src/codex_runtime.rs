//! Optional, read-only evidence for settings omitted by new Codex rollouts.
//! Keep only turn/model/tier/time locally, never prompts or complete log bodies.
use crate::{harness::files, Engine};
use anyhow::Result;
use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Default, Serialize, Deserialize)]
struct Cache {
    cursor: i64,
    evidence: Evidence,
}
#[derive(Clone, Serialize, Deserialize)]
struct Setting {
    tier: Option<String>,
    first_at: i64,
    conflict: bool,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Evidence(BTreeMap<String, Setting>);

#[derive(Default)]
pub(crate) struct Reader {
    connections: HashMap<PathBuf, Option<Connection>>,
    sessions: HashMap<String, Evidence>,
}
impl Reader {
    pub fn read(&mut self, engine: &Engine, path: &Path, thread: &str) -> Result<Evidence> {
        let Some(home) = path
            .ancestors()
            .find(|p| {
                matches!(
                    p.file_name().and_then(|s| s.to_str()),
                    Some("sessions" | "archived_sessions")
                )
            })
            .and_then(Path::parent)
        else {
            return Ok(Evidence::default());
        };
        // Imported logs must never borrow settings from this machine's CODEX_HOME.
        let key = format!(
            "codex_runtime_v1:{}",
            files::hash(json!([home, thread]).to_string().as_bytes())
        );
        if let Some(evidence) = self.sessions.get(&key) {
            return Ok(evidence.clone());
        }
        let mut cache: Cache = engine.lock()?.meta(&key)?.unwrap_or_default();
        let before = serde_json::to_string(&cache)?;
        let conn = self.connections.entry(home.to_owned()).or_insert_with(|| {
            let conn = Connection::open_with_flags(
                home.join("logs_2.sqlite"),
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .ok()?;
            conn.busy_timeout(Duration::from_millis(100)).ok()?;
            Some(conn)
        });
        if let Some(conn) = conn {
            // A missing, busy or incompatible optional database cannot block Token ingestion.
            let mut refreshed = cache.clone();
            if refresh(conn, thread, &mut refreshed).is_ok() {
                cache = refreshed;
            }
        }
        if serde_json::to_string(&cache)? != before {
            engine.lock()?.set_meta(&key, &cache)?;
        }
        self.sessions.insert(key, cache.evidence.clone());
        Ok(cache.evidence)
    }
}
fn refresh(conn: &Connection, thread: &str, cache: &mut Cache) -> Result<()> {
    let max: i64 = conn.query_row(
        "SELECT COALESCE(MAX(id),0) FROM logs WHERE thread_id=?1",
        [thread],
        |r| r.get(0),
    )?;
    if max < cache.cursor {
        cache.cursor = 0;
    }
    let mut stmt = conn.prepare("SELECT id,ts,ts_nanos,feedback_log_body FROM logs WHERE thread_id=?1 AND id>?2 AND id<=?3 AND target='feedback_tags' AND length(feedback_log_body)<=131072 AND feedback_log_body LIKE '%\"service_tier\"%' ORDER BY id")?;
    let rows = stmt.query_map(params![thread, cache.cursor, max], |r| {
        Ok((
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    for row in rows {
        let (seconds, nanos, body) = row?;
        if !(0..1_000_000_000).contains(&nanos) {
            continue;
        }
        let Some(at) = seconds
            .checked_mul(1000)
            .and_then(|s| s.checked_add(nanos / 1_000_000))
        else {
            continue;
        };
        if let Some((turn, model, tier)) = tags(&body, thread) {
            let setting = cache
                .evidence
                .0
                .entry(format!("{turn}\n{model}"))
                .or_insert_with(|| Setting {
                    tier: tier.clone(),
                    first_at: at,
                    conflict: false,
                });
            setting.first_at = setting.first_at.min(at);
            setting.conflict |= setting.tier != tier;
        }
    }
    cache.cursor = max;
    Ok(())
}
// Tracing prefixes contain Rust-style braces before the actual JSON tags.
fn tags(body: &str, thread: &str) -> Option<(String, String, Option<String>)> {
    for (offset, _) in body.match_indices('{').take(256) {
        let Some(Ok(value)) = serde_json::Deserializer::from_str(&body[offset..])
            .into_iter::<Value>()
            .next()
        else {
            continue;
        };
        let Some(raw) = value.get("service_tier") else {
            continue;
        };
        let mut turn = None;
        let mut model = None;
        for token in body[..offset].split(|c: char| c.is_whitespace() || matches!(c, '{' | '}')) {
            let Some((key, value)) = token.split_once('=') else {
                continue;
            };
            let value = value.trim_matches('"');
            match key {
                "thread_id" | "thread.id" | "conversation.id" if value != thread => return None,
                "turn_id" | "turn.id" => consistent(&mut turn, value)?,
                "model" => consistent(&mut model, value)?,
                _ => (),
            }
        }
        let turn = turn.filter(|v| uuid::Uuid::parse_str(v).is_ok())?;
        let model = model.filter(|v| !v.is_empty() && v.len() <= 256)?;
        let tier = raw
            .as_str()
            .filter(|v| *v != "standard" && crate::model::RequestMetadata::valid_tier(v))
            .map(str::to_owned);
        return Some((turn, model, tier));
    }
    None
}
fn consistent(slot: &mut Option<String>, value: &str) -> Option<()> {
    if slot.as_deref().is_some_and(|previous| previous != value) {
        return None;
    }
    *slot = Some(value.to_owned());
    Some(())
}
impl Evidence {
    pub fn fingerprint(&self) -> String {
        files::hash(serde_json::to_string(self).unwrap().as_bytes())
    }
    pub fn enrich(&self, rows: &mut [Value]) {
        let mut turn = None;
        let mut model = None;
        for row in rows {
            let p = &row["payload"];
            if row["type"] == "session_meta"
                || (row["type"] == "event_msg"
                    && matches!(
                        p["type"].as_str(),
                        Some("task_complete" | "task_completed" | "turn_aborted")
                    ))
            {
                turn = None;
                model = None;
            }
            if row["type"] == "event_msg" && p["type"] == "task_started" {
                turn = p["turn_id"].as_str().map(str::to_owned);
                model = None;
            }
            if row["type"] == "turn_context" {
                if let Some(id) = p["turn_id"].as_str() {
                    turn = Some(id.to_owned());
                }
                model = p["model"].as_str().map(str::to_owned);
            }
            if row["type"] != "token_usage_record"
                && !(row["type"] == "event_msg" && p["type"] == "token_count")
            {
                continue;
            }
            let (Some(turn), Some(model)) =
                (p["turn_id"].as_str().or(turn.as_deref()), model.as_deref())
            else {
                continue;
            };
            let Some(at) = row["timestamp"]
                .as_str()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|t| t.timestamp_millis())
            else {
                continue;
            };
            let Some(setting) = self.0.get(&format!("{turn}\n{model}")) else {
                continue;
            };
            if !setting.conflict && at >= setting.first_at {
                if let Some(tier) = &setting.tier {
                    row["_scopeRuntimeTier"] = json!({"model":model,"tier":tier});
                }
            }
        }
    }
}
