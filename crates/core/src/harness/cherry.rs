//! Cherry Studio 2.x SQLite: read a consistent snapshot without touching credentials.
use super::{files, transcript, Descriptor, HarnessAdapter, ParseResult, TimelineIndex};
use crate::model::{ParsedUsage, Session, Tokens};
use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};

pub struct Cherry;

fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(Duration::from_secs(5))?;
    Ok(conn)
}

fn default_db() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_default()
        .join("CherryStudio")
        .join("Data")
        .join("cherrystudio.sqlite")
}

fn events(
    path: &Path,
    session: Option<&str>,
    metadata_only: bool,
    usage_only: bool,
) -> Result<(Vec<Value>, String, Vec<String>, bool)> {
    let mut conn = open(path)?;
    let tx = conn.transaction()?;
    let mut out = Vec::new();
    if usage_only {
        // Project only usage fields and message-to-session relationships; never read API keys or text.
        let mut stmt = tx.prepare("SELECT r.id,r.request_id,r.record_kind,r.request_count,r.message_kind,r.model_id,r.provider_id,r.input_tokens,r.output_tokens,r.total_tokens,r.no_cache_tokens,r.cache_read_tokens,r.cache_write_tokens,r.reasoning_tokens,r.cost,r.cost_currency,r.cost_source,r.created_at,m.topic_id,a.session_id,r.modality FROM ai_usage_record r LEFT JOIN message m ON r.message_kind='chat' AND m.id=r.message_id LEFT JOIN agent_session_message a ON r.message_kind='agent-session' AND a.id=r.message_id ORDER BY r.id")?;
        let rows = stmt.query_map([], |r| Ok(json!({"kind":"usage","id":r.get::<_,String>(0)?,"requestId":r.get::<_,String>(1)?,"recordKind":r.get::<_,String>(2)?,"count":r.get::<_,i64>(3)?,"messageKind":r.get::<_,Option<String>>(4)?,"model":r.get::<_,Option<String>>(5)?,"provider":r.get::<_,Option<String>>(6)?,"input":r.get::<_,Option<i64>>(7)?,"output":r.get::<_,Option<i64>>(8)?,"total":r.get::<_,Option<i64>>(9)?,"noCache":r.get::<_,Option<i64>>(10)?,"read":r.get::<_,Option<i64>>(11)?,"write":r.get::<_,Option<i64>>(12)?,"reasoning":r.get::<_,Option<i64>>(13)?,"cost":r.get::<_,Option<f64>>(14)?,"currency":r.get::<_,Option<String>>(15)?,"costSource":r.get::<_,Option<String>>(16)?,"at":r.get::<_,i64>(17)?,"chat":r.get::<_,Option<String>>(18)?,"agent":r.get::<_,Option<String>>(19)?,"modality":r.get::<_,String>(20)?})))?;
        for row in rows {
            out.push(row?);
        }
    } else {
        for (table, kind, filter) in [
            ("topic", "chat", "WHERE deleted_at IS NULL"),
            ("agent_session", "agent", ""),
        ] {
            let mut stmt = tx.prepare(&format!(
                "SELECT id,name,created_at,last_activity_at FROM {table} {filter} ORDER BY id"
            ))?;
            let rows = stmt.query_map([], |r| Ok(json!({"kind":"session","sessionType":kind,"id":r.get::<_,String>(0)?,"title":r.get::<_,String>(1)?,"created":r.get::<_,i64>(2)?,"updated":r.get::<_,i64>(3)?})))?;
            for row in rows {
                let v = row?;
                if session.is_none_or(|s| {
                    s == format!("{}:{}", kind, v["id"].as_str().unwrap_or_default())
                }) {
                    out.push(v);
                }
            }
        }
        if !metadata_only {
            if let Some((kind, id)) = session.and_then(|s| s.split_once(':')) {
                let (table, foreign, deleted) = match kind {
                    "chat" => ("message", "topic_id", "AND deleted_at IS NULL"),
                    "agent" => ("agent_session_message", "session_id", ""),
                    _ => anyhow::bail!("Invalid Cherry session ID"),
                };
                let mut stmt = tx.prepare(&format!("SELECT id,role,data,created_at FROM {table} WHERE {foreign}=?1 {deleted} ORDER BY created_at,id"))?;
                let rows = stmt.query_map([id], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i64>(3)?,
                    ))
                })?;
                for row in rows {
                    let (id, role, data, at) = row?;
                    let data: Value =
                        serde_json::from_str(&data).context("Invalid Cherry message data")?;
                    anyhow::ensure!(data.is_object(), "Invalid Cherry message data");
                    out.push(json!({"kind":"message","id":id,"role":role,"data":data,"at":at}));
                }
            }
        }
    }
    drop(tx);
    let fingerprint = files::hash(&serde_json::to_vec(&out)?);
    Ok((out, fingerprint, vec![], true))
}

fn parse_usage(e: &Value) -> Result<ParsedUsage> {
    anyhow::ensure!(e["modality"] == "language", "Non-language usage");
    anyhow::ensure!(
        e["recordKind"] == "invocation",
        "Legacy aggregate is not an individual invocation"
    );
    anyhow::ensure!(
        e["count"] == 1,
        "Aggregate request count is not yet supported"
    );
    let n = |key: &str| {
        e[key]
            .as_u64()
            .filter(|v| *v <= 1_000_000_000_000)
            .with_context(|| format!("Missing or invalid {key}"))
    };
    let input = n("input")?;
    let output = n("output")?;
    let total = n("total")?;
    anyhow::ensure!(
        input.checked_add(output) == Some(total),
        "Inconsistent total"
    );
    let fresh = n("noCache")?;
    let optional = |key: &str| -> Result<u64> {
        if e[key].is_null() {
            Ok(0)
        } else {
            n(key)
        }
    };
    let read = optional("read")?;
    let write = optional("write")?;
    anyhow::ensure!(
        fresh.checked_add(read).and_then(|v| v.checked_add(write)) == Some(input),
        "Incomplete input token breakdown"
    );
    let reasoning = n("reasoning")?;
    anyhow::ensure!(
        reasoning <= output && total <= 1_000_000_000_000,
        "Invalid token breakdown"
    );
    let id = e["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .context("Missing usage identity")?;
    let model = e["model"]
        .as_str()
        .filter(|s| !s.is_empty())
        .context("Missing model")?;
    let at = e["at"]
        .as_i64()
        .filter(|v| *v > 0)
        .context("Missing timestamp")?;
    let session = if let Some(s) = e["chat"].as_str() {
        format!("chat:{s}")
    } else if let Some(s) = e["agent"].as_str() {
        format!("agent:{s}")
    } else {
        "unattributed".into()
    };
    // Only a provider-reported USD amount is an actual reported cost. Cherry's computed costs are estimates.
    let reported_cost = if e["costSource"] == "provider" && e["currency"] == "USD" {
        e["cost"]
            .as_f64()
            .filter(|v| v.is_finite() && *v >= 0.0 && *v < 1_000_000_000.0)
            .and_then(|v| {
                let s = v.to_string();
                Decimal::from_str(&s)
                    .or_else(|_| Decimal::from_scientific(&s))
                    .ok()
            })
            .map(|v| v.normalize().to_string())
    } else {
        None
    };
    Ok(ParsedUsage {
        request_metadata: None,
        provider: crate::providers::parse(e["provider"].as_str()),
        id: format!("cherry:{id}"),
        session_id: session,
        native_event_id: id.into(),
        occurred_at: at / 1000,
        model: model.into(),
        tokens: Tokens {
            input: fresh,
            output,
            cache_read: read,
            cache_write: write,
            reasoning,
            cache_write1h: 0,
        },
        reported_cost,
        timing: None,
    })
}

impl HarnessAdapter for Cherry {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: "cherry",
            name: "Cherry Studio",
            parser_version: "cherry-sqlite-v3-pricing",
            default_roots: vec![default_db().to_string_lossy().into()],
            usage: true,
            transcript: true,
            delete: false,
        }
    }
    fn discover(&self, root: &Path) -> Result<Vec<PathBuf>> {
        let p = if root.is_file() {
            root.to_owned()
        } else if root.join("cherrystudio.sqlite").is_file() {
            root.join("cherrystudio.sqlite")
        } else {
            root.join("Data").join("cherrystudio.sqlite")
        };
        anyhow::ensure!(
            p.is_file(),
            "Cherry Studio database not found: {}",
            p.display()
        );
        Ok(vec![p])
    }
    fn read_source(&self, path: &Path) -> Result<(Vec<Value>, String, Vec<String>, bool)> {
        events(path, None, true, false)
    }
    fn read_usage_source(&self, path: &Path) -> Result<(Vec<Value>, String, Vec<String>, bool)> {
        events(path, None, false, true)
    }
    fn read_metadata(&self, path: &Path) -> Result<Vec<Session>> {
        Ok(self.sessions(path, &events(path, None, true, false)?.0))
    }
    fn read_messages(&self, path: &Path, session: Option<&str>) -> Result<Vec<Value>> {
        let session = session.context("Missing Cherry session ID")?;
        Ok(self.messages(&events(path, Some(session), false, false)?.0))
    }
    fn parse(&self, _: &Path, rows: &[Value], _: &TimelineIndex) -> Result<ParseResult> {
        let mut result = ParseResult {
            complete: true,
            ..Default::default()
        };
        let mut deferred = BTreeMap::<String, usize>::new();
        for e in rows.iter().filter(|e| e["kind"] == "usage") {
            // These records are outside the supported accounting scope, rather
            // than incomplete reads. In particular a legacy aggregate must not
            // prevent a source checkpoint or block unrelated session deletion.
            if e["recordKind"] == "legacy-aggregate"
                || matches!(
                    e["modality"].as_str(),
                    Some("embedding" | "image" | "rerank")
                )
            {
                continue;
            }
            match parse_usage(e) {
                Ok(u) => result.usage.push(u),
                Err(error) => {
                    *deferred.entry(error.to_string()).or_default() += 1;
                    result.complete = false;
                }
            }
        }
        for (reason, count) in deferred {
            result.diagnostics.push(format!(
                "Cherry Studio: {count} deferred usage records: {reason}"
            ));
        }
        Ok(result)
    }
    fn session(&self, path: &Path, events: &[Value]) -> Option<Session> {
        self.sessions(path, events).into_iter().next()
    }
    fn sessions(&self, path: &Path, events: &[Value]) -> Vec<Session> {
        events
            .iter()
            .filter(|e| e["kind"] == "session")
            .filter_map(|e| {
                Some(Session {
                    provider_id: "cherry".into(),
                    session_id: format!("{}:{}", e["sessionType"].as_str()?, e["id"].as_str()?),
                    title: e["title"].as_str().map(str::to_owned),
                    summary: None,
                    project_dir: None,
                    created_at: e["created"].as_i64(),
                    last_active_at: e["updated"].as_i64(),
                    source_path: Some(path.to_string_lossy().into()),
                    resume_command: None,
                    can_delete: false,
                })
            })
            .collect()
    }
    fn messages(&self, events: &[Value]) -> Vec<Value> {
        events.iter().filter(|e|e["kind"]=="message" && matches!(e["role"].as_str(),Some("user"|"assistant"))).map(|e|{let mut blocks=Vec::new(); for p in e["data"]["parts"].as_array().into_iter().flatten(){match p["type"].as_str(){Some("text")=>blocks.push(json!({"type":"text","text":p["text"].as_str().unwrap_or("")})),Some("reasoning")=>blocks.push(json!({"type":"thinking","text":p["text"].as_str().unwrap_or("")})),Some("dynamic-tool")=>{let id=p["toolCallId"].as_str().unwrap_or("");let name=p["toolName"].as_str().unwrap_or("tool");blocks.push(transcript::call(id,name,&p["input"]));if !p["output"].is_null(){blocks.push(transcript::result(id,&p["output"],false));}},_=>{}}} let content=blocks.iter().filter(|b|b["type"]=="text").filter_map(|b|b["text"].as_str()).collect::<Vec<_>>().join("\n");json!({"id":e["id"],"role":e["role"],"ts":e["at"],"content":content,"blocks":blocks})}).collect()
    }
}
