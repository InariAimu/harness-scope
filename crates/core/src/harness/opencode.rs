//! Read-only OpenCode SQLite snapshots. V2 supersedes migrated V1 sessions.
use super::{files, transcript, Descriptor, HarnessAdapter, ParseResult, TimelineIndex};
use crate::model::{ParsedUsage, Session, Tokens, UsageTiming};
use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};

pub struct OpenCode;

fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".local/share"))
        .join("opencode")
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [name],
        |r| r.get(0),
    )?)
}

fn counter(value: &Value, label: &str, required: bool) -> Result<u64> {
    if value.is_null() && !required {
        return Ok(0);
    }
    value
        .as_u64()
        .filter(|n| *n <= 1_000_000_000_000)
        .with_context(|| format!("Missing or invalid OpenCode counter: {label}"))
}

fn part_start(part: &Value) -> Option<i64> {
    if !matches!(part["type"].as_str(), Some("text" | "reasoning")) {
        return None;
    }
    part["time"]["created"]
        .as_i64()
        .or_else(|| part["time"]["start"].as_i64())
}

fn parse_usage(row: &Value, part_starts: Option<&Vec<i64>>) -> Result<Option<ParsedUsage>> {
    let d = &row["data"];
    if d["role"] != "assistant" {
        return Ok(None);
    }
    // Partial streaming messages must be retried, never marked as a complete source.
    anyhow::ensure!(
        d["time"]["completed"].as_i64().is_some(),
        "Assistant message is still incomplete"
    );
    let u = &d["tokens"];
    let reasoning = counter(&u["reasoning"], "reasoning", false)?;
    // OpenCode output excludes reasoning; our output includes that subset.
    let tokens = Tokens {
        input: counter(&u["input"], "input", true)?,
        output: counter(&u["output"], "output", true)?
            .checked_add(reasoning)
            .context("Output overflow")?,
        cache_read: counter(&u["cache"]["read"], "cache.read", false)?,
        cache_write: counter(&u["cache"]["write"], "cache.write", false)?,
        cache_write1h: 0,
        reasoning,
    };
    anyhow::ensure!(
        tokens.total() <= 1_000_000_000_000,
        "OpenCode total exceeds counter limit"
    );
    let reported_cost = if d["cost"].is_null() {
        None
    } else {
        let raw = d["cost"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| d["cost"].to_string());
        let amount = Decimal::from_str(&raw)
            .or_else(|_| Decimal::from_scientific(&raw))
            .context("Invalid OpenCode reported cost")?;
        anyhow::ensure!(
            amount >= Decimal::ZERO && amount < Decimal::from(1_000_000_000u64),
            "Invalid OpenCode reported cost"
        );
        Some(amount.normalize().to_string())
    };
    if tokens.total() == 0 {
        return Ok(None);
    }
    let id = row["id"].as_str().context("Missing message ID")?;
    let session = row["sessionId"].as_str().context("Missing session ID")?;
    let created = d["time"]["created"]
        .as_i64()
        .filter(|t| *t > 0)
        .context("Missing message timestamp")?;
    let model = d["modelID"]
        .as_str()
        .or_else(|| d["model"]["id"].as_str())
        .filter(|s| !s.is_empty())
        .context("Missing OpenCode model")?;
    let completed = d["time"]["completed"].as_i64();
    let streamed = d["time"]["streamed"].as_i64().filter(|end| *end > created);
    let (end, source) = if let Some(end) = streamed {
        (Some(end), "opencode_time_streamed")
    } else {
        (completed, "opencode_time_completed")
    };
    let first = d["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(part_start)
        .chain(part_starts.into_iter().flatten().copied())
        .filter(|t| *t >= created && end.is_some_and(|end| *t <= end))
        .min();
    let timing = end.and_then(|end| {
        UsageTiming::estimate(created, end, first, source, "opencode_content_start")
    });
    Ok(Some(ParsedUsage {
        request_metadata: None,
        provider: crate::providers::parse(
            d["providerID"]
                .as_str()
                .or_else(|| d["model"]["providerID"].as_str()),
        ),
        id: format!("opencode:{session}:{id}"),
        session_id: session.into(),
        native_event_id: id.into(),
        occurred_at: created / 1000,
        model: model.into(),
        tokens,
        reported_cost,
        timing,
    }))
}

impl HarnessAdapter for OpenCode {
    fn descriptor(&self) -> Descriptor {
        let root = std::env::var_os("OPENCODE_DB")
            .map(|p| {
                let p = PathBuf::from(p);
                if p.is_absolute() {
                    p
                } else {
                    data_dir().join(p)
                }
            })
            .unwrap_or_else(data_dir);
        Descriptor {
            id: "opencode",
            name: "OpenCode",
            parser_version: "opencode-sqlite-v3-provider",
            default_roots: vec![root.to_string_lossy().into()],
            usage: true,
            transcript: true,
            delete: false,
        }
    }

    fn discover(&self, root: &Path) -> Result<Vec<PathBuf>> {
        anyhow::ensure!(
            root.exists(),
            "OpenCode source does not exist: {}",
            root.display()
        );
        let path = if root.is_file() {
            root.to_owned()
        } else {
            root.join("opencode.db")
        };
        anyhow::ensure!(
            path.is_file(),
            "OpenCode database not found: {}",
            path.display()
        );
        Ok(vec![path])
    }

    fn read_source(&self, path: &Path) -> Result<(Vec<Value>, String, Vec<String>, bool)> {
        read_events(path, None, false)
    }
    fn read_metadata(&self, path: &Path) -> Result<Vec<Session>> {
        Ok(self.sessions(path, &read_events(path, None, true)?.0))
    }
    fn read_messages(&self, path: &Path, session: Option<&str>) -> Result<Vec<Value>> {
        let session = session.context("Missing OpenCode session ID")?;
        let (events, _, errors, complete) = read_events(path, Some(session), false)?;
        anyhow::ensure!(
            complete && errors.is_empty(),
            "Session contains incomplete or invalid data"
        );
        Ok(self.messages_for_session(&events, Some(session)))
    }

    fn parse(&self, _: &Path, events: &[Value], _: &TimelineIndex) -> Result<ParseResult> {
        let mut out = ParseResult {
            complete: true,
            ..Default::default()
        };
        let mut part_starts: HashMap<(&str, &str), Vec<i64>> = HashMap::new();
        for part in events.iter().filter(|e| e["kind"] == "part") {
            if let (Some(session), Some(message), Some(at)) = (
                part["sessionId"].as_str(),
                part["messageId"].as_str(),
                part_start(&part["data"]),
            ) {
                part_starts.entry((session, message)).or_default().push(at);
            }
        }
        for event in events.iter().filter(|e| e["kind"] == "message") {
            let key = (
                event["sessionId"].as_str().unwrap_or_default(),
                event["id"].as_str().unwrap_or_default(),
            );
            match parse_usage(event, part_starts.get(&key)) {
                Ok(Some(usage)) => out.usage.push(usage),
                Ok(None) => {}
                Err(e) => {
                    out.complete = false;
                    out.diagnostics
                        .push(format!("OpenCode {}: {e}", event["id"]));
                }
            }
        }
        Ok(out)
    }

    fn session(&self, path: &Path, events: &[Value]) -> Option<Session> {
        self.sessions(path, events).into_iter().next()
    }

    fn sessions(&self, path: &Path, events: &[Value]) -> Vec<Session> {
        events
            .iter()
            .filter(|e| e["kind"] == "session")
            .filter_map(|e| {
                let id = e["id"].as_str()?;
                let safe = id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
                Some(Session {
                    provider_id: "opencode".into(),
                    session_id: id.into(),
                    title: e["title"].as_str().map(str::to_owned),
                    summary: None,
                    project_dir: e["directory"].as_str().map(str::to_owned),
                    created_at: e["created"].as_i64(),
                    last_active_at: e["updated"].as_i64(),
                    source_path: Some(path.to_string_lossy().into()),
                    resume_command: safe.then(|| format!("opencode --session {id}")),
                    can_delete: false,
                })
            })
            .collect()
    }

    fn messages(&self, _: &[Value]) -> Vec<Value> {
        Vec::new()
    }

    fn messages_for_session(&self, events: &[Value], session: Option<&str>) -> Vec<Value> {
        let mut messages = Vec::new();
        for e in events
            .iter()
            .filter(|e| e["kind"] == "message" && e["sessionId"].as_str() == session)
        {
            let d = &e["data"];
            let role = d["role"].as_str().unwrap_or_default();
            if !["user", "assistant"].contains(&role) {
                continue;
            }
            let mut blocks = Vec::new();
            if let Some(text) = d["text"].as_str() {
                blocks.push(json!({"type":"text","text":text}));
            }
            let parts: Vec<&Value> = if let Some(content) = d["content"].as_array() {
                content.iter().collect()
            } else {
                events
                    .iter()
                    .filter(|p| {
                        p["kind"] == "part"
                            && p["messageId"] == e["id"]
                            && p["sessionId"] == e["sessionId"]
                    })
                    .map(|p| &p["data"])
                    .collect()
            };
            for p in parts {
                match p["type"].as_str().unwrap_or("") {
                    "text" => blocks.push(json!({"type":"text","text":p["text"]})),
                    "reasoning" => blocks.push(json!({"type":"thinking","text":p["text"]})),
                    "tool" => {
                        let id = p["callID"]
                            .as_str()
                            .or_else(|| p["id"].as_str())
                            .unwrap_or("");
                        let state = &p["state"];
                        let name = p["tool"]
                            .as_str()
                            .or_else(|| p["name"].as_str())
                            .unwrap_or("tool");
                        blocks.push(transcript::call(id, name, &state["input"]));
                        if state["status"] == "completed" || state["status"] == "error" {
                            let output = state
                                .get("output")
                                .or_else(|| state.get("content"))
                                .or_else(|| state.get("error"))
                                .unwrap_or(&Value::Null);
                            let mut result =
                                transcript::result(id, output, state["status"] == "error");
                            result["status"] = json!(if state["status"] == "error" {
                                "error"
                            } else {
                                "success"
                            });
                            blocks.push(result);
                        }
                    }
                    _ => {}
                }
            }
            let content = blocks
                .iter()
                .filter(|b| b["type"] == "text")
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let output = d["tokens"]["output"]
                .as_u64()
                .and_then(|n| n.checked_add(d["tokens"]["reasoning"].as_u64().unwrap_or(0)));
            messages.push(json!({"id":e["id"],"role":role,"ts":d["time"]["created"],"content":content,"blocks":blocks,
                "meta":{"model":d.get("modelID").unwrap_or(&d["model"]["id"]),"inputTokens":d["tokens"]["input"],"outputTokens":output}}));
        }
        messages
    }
}

fn read_events(
    path: &Path,
    session: Option<&str>,
    metadata_only: bool,
) -> Result<(Vec<Value>, String, Vec<String>, bool)> {
    let header_filter = if session.is_some() { "WHERE id=?1" } else { "" };
    let message_filter = if session.is_some() {
        "WHERE session_id=?1"
    } else {
        ""
    };
    let mut conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(Duration::from_secs(5))?;
    let tx = conn.transaction()?;
    let v1 = table_exists(&tx, "message")?;
    let v2 = table_exists(&tx, "session_message")?;
    let sessions_v1 = table_exists(&tx, "session")?;
    let sessions_v2 = table_exists(&tx, "session_v2")?;
    anyhow::ensure!(
        (sessions_v1 || sessions_v2) && (v1 || v2),
        "Unsupported OpenCode SQLite schema"
    );
    let mut events = Vec::new();
    let mut diagnostics = Vec::new();
    let mut session_ids = HashSet::new();
    for (table, exists) in [("session_v2", sessions_v2), ("session", sessions_v1)] {
        if !exists {
            continue;
        }
        let mut stmt = tx.prepare(&format!(
                "SELECT id,title,directory,time_created,time_updated FROM {table} {header_filter} ORDER BY id",
            ))?;
        let rows = stmt.query_map(rusqlite::params_from_iter(session), |r| Ok(json!({"kind":"session","id":r.get::<_,String>(0)?,"title":r.get::<_,Option<String>>(1)?,"directory":r.get::<_,String>(2)?,"created":r.get::<_,i64>(3)?,"updated":r.get::<_,i64>(4)?})))?;
        for row in rows {
            let row = row?;
            if session_ids.insert(row["id"].as_str().unwrap_or_default().to_owned()) {
                events.push(row);
            }
        }
    }
    let mut migrated = HashSet::new();
    if let Some(id) = session {
        if sessions_v2
            && tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM session_v2 WHERE id=?1)",
                [id],
                |r| r.get::<_, bool>(0),
            )?
        {
            migrated.insert(id.to_owned());
        }
    }
    if v2 && !metadata_only {
        let mut stmt = tx.prepare(&format!(
                "SELECT id,session_id,type,data FROM session_message {message_filter} ORDER BY session_id,seq,id"
            ))?;
        let rows = stmt.query_map(rusqlite::params_from_iter(session), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (id, session, role, data) = row?;
            migrated.insert(session.clone());
            match serde_json::from_str::<Value>(&data) {
                Ok(mut data) if data.is_object() => {
                    data["role"] = json!(role);
                    events.push(json!({"kind":"message","id":id,"sessionId":session,"data":data}));
                }
                Ok(_) => diagnostics.push(format!("OpenCode message {id}: expected an object")),
                Err(e) => diagnostics.push(format!("OpenCode message {id}: {e}")),
            }
        }
    }
    if v1 && !metadata_only {
        let mut stmt = tx.prepare(&format!(
                "SELECT id,session_id,data FROM message {message_filter} ORDER BY session_id,time_created,id"
            ))?;
        let rows = stmt.query_map(rusqlite::params_from_iter(session), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (id, session, data) = row?;
            if migrated.contains(&session) {
                continue;
            }
            match serde_json::from_str::<Value>(&data) {
                Ok(data) if data.is_object() => {
                    events.push(json!({"kind":"message","id":id,"sessionId":session,"data":data}))
                }
                Ok(_) => diagnostics.push(format!("OpenCode message {id}: expected an object")),
                Err(e) => diagnostics.push(format!("OpenCode message {id}: {e}")),
            }
        }
        if table_exists(&tx, "part")? {
            let mut stmt = tx.prepare(&format!("SELECT id,message_id,session_id,data FROM part {message_filter} ORDER BY session_id,time_created,id"))?;
            let rows = stmt.query_map(rusqlite::params_from_iter(session), |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?;
            for row in rows {
                let (id, message, session, data) = row?;
                if migrated.contains(&session) {
                    continue;
                }
                match serde_json::from_str::<Value>(&data) {
                        Ok(data) if data.is_object() => events.push(json!({"kind":"part","id":id,"messageId":message,"sessionId":session,"data":data})),
                        Ok(_) => diagnostics.push(format!("OpenCode part {id}: expected an object")),
                        Err(e) => diagnostics.push(format!("OpenCode part {id}: {e}")),
                    }
            }
        }
    }
    tx.commit()?;
    let fingerprint = files::hash(&serde_json::to_vec(&events)?);
    let complete = diagnostics.is_empty();
    Ok((events, fingerprint, diagnostics, complete))
}
