//! Antigravity 2.x native SQLite conversations. gen_metadata is the accounting
//! authority; steps repeat the same usage and must never be added a second time.
mod wire;
use super::{google_local, transcript, Descriptor, HarnessAdapter, ParseResult, TimelineIndex};
use crate::model::{Session, UsageTiming};
use anyhow::{ensure, Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};
use wire::Message;

pub struct Antigravity;

// Native protobuf Duration. An absent message is unknown; a present empty
// message is a reported zero. Negative, overflowing and noncanonical values
// must not become a plausible first-token boundary.
fn duration_ms(chat: &Message<'_>, field: u32) -> Result<Option<i64>> {
    if !chat.has(field) {
        return Ok(None);
    }
    let duration = chat.message(field)?;
    let seconds = duration.number(1)?;
    let nanos = duration.number(2)?;
    if seconds > 3600 || nanos >= 1_000_000_000 {
        return Ok(None);
    }
    let ms = seconds * 1000 + (nanos + 500_000) / 1_000_000;
    Ok((ms <= 3_600_000).then_some(ms as i64))
}

fn native_timing(chat: &Message<'_>) -> Option<UsageTiming> {
    // streaming_duration measures first token -> stream end, independently of
    // time_to_first_token. The sum is the request duration used by the reader.
    let first = duration_ms(chat, 11).ok()??;
    let stream = duration_ms(chat, 12).ok()??;
    let total = first.checked_add(stream)?;
    if !(100..=3_600_000).contains(&total) {
        return None;
    }
    Some(UsageTiming {
        duration_ms: total,
        first_token_ms: Some(first),
        duration_source: "antigravity_chat_model".into(),
        first_token_source: Some("antigravity_chat_model".into()),
    })
}
fn open(path: &Path) -> Result<Connection> {
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    db.busy_timeout(Duration::from_secs(5))?;
    Ok(db)
}
fn timestamp(metadata: &Message<'_>) -> Result<Option<i64>> {
    let t = metadata.message(1)?;
    if !t.has(1) {
        return Ok(None);
    }
    let seconds = i64::try_from(t.number(1)?)?;
    Ok(chrono::DateTime::from_timestamp(seconds, 0)
        .filter(|_| seconds > 0)
        .map(|_| seconds))
}

fn read(
    path: &Path,
    usage_only: bool,
    metadata_only: bool,
) -> Result<(Vec<Value>, String, Vec<String>, bool)> {
    let mut db = open(path)?;
    let tx = db.transaction()?;
    let (trajectory, cascade): (String, String) = tx
        .query_row(
            "SELECT trajectory_id,cascade_id FROM trajectory_meta LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .context("Unsupported Antigravity database; expected native SQLite conversations")?;
    let session = if cascade.is_empty() {
        &trajectory
    } else {
        &cascade
    };
    ensure!(
        !session.is_empty(),
        "Missing Antigravity conversation identity"
    );
    let mut steps = BTreeMap::new();
    let mut times = Vec::new();
    let mut messages = Vec::new();
    let mut errors = Vec::new();
    let sql = if usage_only || metadata_only {
        "SELECT idx,metadata,NULL,step_format FROM steps ORDER BY idx"
    } else {
        "SELECT idx,metadata,step_payload,step_format FROM steps ORDER BY idx"
    };
    let mut stmt = tx.prepare(sql)?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Option<Vec<u8>>>(1)?,
            r.get::<_, Option<Vec<u8>>>(2)?,
            r.get::<_, i64>(3)?,
        ))
    })? {
        let (idx, raw, payload, format) = row?;
        ensure!(format == 0, "Unsupported Antigravity step encoding");
        if let Some(raw) = raw {
            let metadata = Message::parse(&raw)?;
            if let Some(at) = timestamp(&metadata)? {
                times.push(at);
            }
            if let Some(payload) = payload {
                if let Some(message) = message(idx, &metadata, &payload)? {
                    messages.push(message);
                }
            }
            steps.insert(idx, raw);
        }
    }
    let mut usage = Vec::new();
    if !metadata_only {
        let mut stmt = tx.prepare("SELECT idx,data FROM gen_metadata ORDER BY idx")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))? {
            let (idx, raw) = row?;
            match generation(idx, &raw, &steps, session) {
                Ok(Some(v)) => usage.push(v),
                Ok(None) => {}
                Err(error) => {
                    errors.push(format!("Antigravity: deferred generation {idx}: {error}"))
                }
            }
        }
    }
    let mut value = json!({"sessionId":session,"usage":usage});
    if !usage_only {
        value["createdAt"] = json!(times.iter().min().map(|n| n * 1000));
        value["lastActiveAt"] = json!(times.iter().max().map(|n| n * 1000));
        value["messages"] = json!(messages);
    }
    let out = vec![value];
    let hash = google_local::fingerprint(&out)?;
    Ok((out, hash, errors.clone(), errors.is_empty()))
}

fn generation(
    idx: i64,
    raw: &[u8],
    steps: &BTreeMap<i64, Vec<u8>>,
    session: &str,
) -> Result<Option<Value>> {
    let generator = Message::parse(raw)?;
    if generator.has(7) {
        return Ok(None);
    } // Explicitly injected, not an API call.
    ensure!(generator.has(1), "No native chat model metadata");
    let chat = generator.message(1)?;
    ensure!(chat.has(4), "Generation has no reported usage yet");
    let usage = chat.message(4)?;
    // Proto3 omits known zero scalar fields. An absent usage MESSAGE, unlike
    // an omitted scalar within that message, is unavailable rather than zero.
    let mut model = chat.text(22)?;
    if model.is_empty() {
        model = chat.text(19)?;
    }
    if model.is_empty() {
        model = chat.text(21)?;
    }
    let referenced = generator.numbers(2)?;
    let mut at = None;
    let mut origin = None;
    for index in referenced {
        if let Some(raw) = i64::try_from(index).ok().and_then(|i| steps.get(&i)) {
            let metadata = Message::parse(raw)?;
            if at.is_none() {
                at = timestamp(&metadata)?;
            }
            if metadata.has(20) {
                let source = metadata.message(20)?;
                let id = if !source.text(4)?.is_empty() {
                    source.text(4)?
                } else {
                    source.text(1)?
                };
                if !id.is_empty() {
                    ensure!(
                        origin.as_deref().is_none_or(|old| old == id),
                        "Conflicting source conversation identities"
                    );
                    origin = Some(id.to_owned());
                }
            }
        }
    }
    let native = [usage.text(7)?, usage.text(11)?, usage.text(12)?]
        .into_iter()
        .find(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("generation-{idx}"));
    Ok(Some(
        json!({"id":native,"originSession":origin.as_deref().unwrap_or(session),"model":model,"at":at,"input":usage.number(2)?,"output":usage.number(3)?,"cacheWrite":usage.number(4)?,"cacheRead":usage.number(5)?,"reasoning":usage.number(9)?,"timing":native_timing(&chat)}),
    ))
}

fn message(idx: i64, metadata: &Message<'_>, payload: &[u8]) -> Result<Option<Value>> {
    let step = Message::parse(payload)?;
    let mut blocks = Vec::new();
    let mut role = "assistant";
    if step.has(19) {
        role = "user";
        let user = step.message(19)?;
        let text = if !user.text(2)?.is_empty() {
            user.text(2)?
        } else {
            user.text(1)?
        };
        blocks.push(json!({"type":"text","text":text}));
    } else if step.has(20) {
        let response = step.message(20)?;
        if !response.text(3)?.is_empty() {
            blocks.push(json!({"type":"thinking","text":response.text(3)?}));
        }
        let text = if response.has(8) {
            response.text(8)?
        } else {
            response.text(1)?
        };
        if !text.is_empty() {
            blocks.push(json!({"type":"text","text":text}));
        }
    } else if step.has(94) {
        blocks.push(json!({"type":"text","text":step.message(94)?.text(2)?}));
    } else if metadata.has(4) {
        // Tool invocations remain visible even when a plugin's result format is
        // unknown. Do not claim an empty or successful result for those plugins.
        let call = metadata.message(4)?;
        blocks.push(transcript::call(
            call.text(1)?,
            call.text(2)?,
            &json!(call.text(3)?),
        ));
    }
    if blocks.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        json!({"id":format!("step-{idx}"),"role":role,"ts":timestamp(metadata)?.map(|n|n*1000),"content":"","blocks":blocks}),
    ))
}

impl HarnessAdapter for Antigravity {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: "antigravity",
            name: "Antigravity",
            parser_version: "antigravity-sqlite-v2-timing",
            default_roots: vec![dirs::home_dir()
                .unwrap_or_default()
                .join(".gemini/antigravity/conversations")
                .to_string_lossy()
                .into()],
            usage: true,
            transcript: true,
            delete: false,
        }
    }
    fn syncs_timing(&self) -> bool {
        // Existing sync servers do not accept this native timing source yet.
        false
    }
    fn discover(&self, root: &Path) -> Result<Vec<PathBuf>> {
        ensure!(
            root.exists(),
            "Antigravity conversation directory does not exist"
        );
        if root.is_file() {
            ensure!(root.extension().is_some_and(|e|e == "db") && root.file_name().is_none_or(|n|n != "conversation_summaries.db"), "Only native Antigravity conversation .db files are supported; legacy .pb files require migration");
            return Ok(vec![root.into()]);
        }
        let root = if root.join("conversations").is_dir() {
            root.join("conversations")
        } else {
            root.into()
        };
        let mut paths = Vec::new();
        let mut legacy = false;
        for entry in std::fs::read_dir(root)? {
            let entry = entry?;
            if entry.file_type()?.is_file()
                && entry.path().extension().is_some_and(|e| e == "db")
                && entry.file_name() != "conversation_summaries.db"
            {
                paths.push(entry.path());
            } else if entry.path().extension().is_some_and(|e| e == "pb") {
                legacy = true;
            }
        }
        ensure!(!paths.is_empty() || !legacy, "Legacy Antigravity .pb conversation format is unsupported; migrate with Antigravity first");
        paths.sort();
        Ok(paths)
    }
    fn read_source(&self, path: &Path) -> Result<(Vec<Value>, String, Vec<String>, bool)> {
        read(path, false, false)
    }
    fn read_usage_source(&self, path: &Path) -> Result<(Vec<Value>, String, Vec<String>, bool)> {
        read(path, true, false)
    }
    fn usage_reader_version(&self) -> &'static str {
        "antigravity-projection-v2-timing"
    }
    fn read_metadata(&self, path: &Path) -> Result<Vec<Session>> {
        Ok(self.sessions(path, &read(path, false, true)?.0))
    }
    fn enrich_metadata(&self, root: &Path, sessions: &mut [Session]) {
        let folder = if root.is_file() {
            root.parent().unwrap_or(root)
        } else {
            root
        };
        let summary = if folder.join("conversation_summaries.db").is_file() {
            folder.join("conversation_summaries.db")
        } else {
            folder
                .parent()
                .unwrap_or(folder)
                .join("conversation_summaries.db")
        };
        let Ok(db) = open(&summary) else {
            return;
        };
        for s in sessions {
            if let Ok(title) = db.query_row(
                "SELECT title FROM conversation_summaries WHERE conversation_id=?1",
                [&s.session_id],
                |r| r.get::<_, String>(0),
            ) {
                if !title.is_empty() {
                    s.title = Some(title);
                }
            }
        }
    }
    fn parse(&self, _: &Path, values: &[Value], _: &TimelineIndex) -> Result<ParseResult> {
        google_local::parse("antigravity", values)
    }
    fn session(&self, path: &Path, values: &[Value]) -> Option<Session> {
        google_local::session("antigravity", path, values)
    }
    fn messages(&self, values: &[Value]) -> Vec<Value> {
        google_local::messages(values)
    }
}
