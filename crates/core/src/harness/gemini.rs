//! Gemini CLI native chat recordings: legacy JSON and append-only JSONL.
//! Accounting retains billed messages removed by rewind; the reader follows the
//! active conversation. No prompts or tool output enter the usage cache.
use super::{
    files, google_local, transcript, Descriptor, HarnessAdapter, ParseResult, TimelineIndex,
};
use crate::model::Session;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};

pub struct Gemini;

fn supported(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
        n.starts_with("session-") && (n.ends_with(".json") || n.ends_with(".jsonl"))
    })
}

fn remember(message: &Value, usage: &mut BTreeMap<String, Value>) -> bool {
    if !message["id"].as_str().is_some_and(|id| !id.is_empty())
        || !matches!(
            message["type"].as_str(),
            Some("user" | "gemini" | "info" | "error" | "warning")
        )
    {
        return false;
    }
    if message["type"] == "gemini" {
        if let Some(id) = message["id"].as_str() {
            // The CLI appends the same message again when tokens arrive.
            // An unfinished replay must not replace a previously completed call.
            if !message["tokens"].is_null() || !usage.contains_key(id) {
                usage.insert(id.into(), json!({"id":id,"timestamp":message["timestamp"],"model":message["model"],"tokens":message["tokens"]}));
            }
        }
    }
    true
}

fn upsert(messages: &mut Vec<Value>, message: Value) {
    if let Some(old) = messages.iter_mut().find(|m| m["id"] == message["id"]) {
        *old = message;
    } else {
        messages.push(message);
    }
}

fn patch(messages: &mut [Value], p: &Value) {
    if let Some(message) = messages.iter_mut().find(|m| m["id"] == p["id"]) {
        if let Some(content) = p.get("content") {
            message["content"] = content.clone();
        }
        for update in p["toolCalls"].as_array().into_iter().flatten() {
            if let Some(call) = message["toolCalls"]
                .as_array_mut()
                .and_then(|calls| calls.iter_mut().find(|c| c["id"] == update["id"]))
            {
                if let Some(result) = update.get("result") {
                    call["result"] = result.clone();
                }
            }
        }
    }
}

fn read(path: &Path, usage_only: bool) -> Result<(Vec<Value>, String, Vec<String>, bool)> {
    ensure!(
        supported(path),
        "Expected a Gemini CLI session-*.json or .jsonl file"
    );
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 64 * 1024 * 1024,
        "Gemini session exceeds 64 MiB"
    );
    let mut meta = json!({});
    let mut active = Vec::<Value>::new();
    let mut usage = BTreeMap::new();
    let mut errors = Vec::new();
    let records = if path.extension().is_some_and(|e| e == "json") {
        let value =
            serde_json::from_slice::<Value>(&bytes).context("Incomplete Gemini JSON session")?;
        ensure!(
            value["messages"].is_array(),
            "Missing Gemini conversation messages"
        );
        vec![value]
    } else {
        let mut records = Vec::new();
        for (i, line) in bytes.split(|b| *b == b'\n').enumerate() {
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            match serde_json::from_slice::<Value>(line) {
                Ok(v) => records.push(v),
                Err(_) => errors.push(format!(
                    "Gemini CLI: invalid or incomplete JSONL record {}",
                    i + 1
                )),
            }
        }
        records
    };
    for record in records {
        let update = record.get("$set").unwrap_or(&record);
        if update.get("sessionId").is_some() || record.get("$set").is_some() {
            if let (Some(old), Some(new)) =
                (meta["sessionId"].as_str(), update["sessionId"].as_str())
            {
                ensure!(old == new, "Mixed Gemini session identities");
            }
            for key in [
                "sessionId",
                "startTime",
                "lastUpdated",
                "summary",
                "directories",
            ] {
                if let Some(v) = update.get(key) {
                    meta[key] = v.clone();
                }
            }
            if let Some(messages) = update["messages"].as_array() {
                active = messages.clone();
                for m in messages {
                    if !remember(m, &mut usage) {
                        errors.push("Gemini CLI: invalid native message identity or type".into());
                    }
                }
            } else if update.get("messages").is_some() {
                errors.push("Gemini CLI: invalid messages checkpoint".into());
            }
        } else if let Some(id) = record["$rewindTo"].as_str() {
            let end = active.iter().position(|m| m["id"] == id).unwrap_or(0);
            active.truncate(end);
        } else if let Some(p) = record.get("$patch") {
            patch(&mut active, p);
            for update in p["updates"].as_array().into_iter().flatten() {
                patch(&mut active, update);
            }
            if let Some(ids) = p["removeIds"].as_array() {
                active.retain(|m| !ids.contains(&m["id"]));
            }
            if let Some(ids) = p["orderIds"].as_array() {
                let mut ordered = Vec::new();
                for id in ids {
                    if let Some(m) = active.iter().find(|m| m["id"] == *id) {
                        ordered.push(m.clone());
                    }
                }
                for m in &active {
                    if !ids.contains(&m["id"]) {
                        ordered.push(m.clone());
                    }
                }
                active = ordered;
            }
        } else if record["id"].is_string() && record["type"].is_string() {
            if !remember(&record, &mut usage) {
                errors.push("Gemini CLI: invalid native message identity or type".into());
            }
            upsert(&mut active, record);
        } else {
            errors.push("Gemini CLI: unrecognized session record".into());
        }
    }
    ensure!(
        meta["sessionId"].as_str().is_some_and(|s| !s.is_empty()),
        "Missing Gemini session identity"
    );
    let records: Vec<_> = usage.into_values().map(|m| {
        let t = &m["tokens"];
        let count = |key: &str, optional:bool| match t.get(key) { None if optional => Some(0), Some(v) => v.as_u64(), _ => None };
        let mut input = count("input",false).zip(count("cached",false)).and_then(|(a,b)| a.checked_sub(b)).zip(count("tool",true)).and_then(|(a,b)|a.checked_add(b));
        let output = count("output",false).zip(count("thoughts",true)).and_then(|(a,b)| a.checked_add(b));
        let computed = input.zip(count("cached",false)).and_then(|(a,b)|a.checked_add(b)).zip(output).and_then(|(a,b)|a.checked_add(b));
        if computed.is_none() || computed != count("total",false) { input = None; }
        json!({"id":m["id"],"at":files::timestamp(&m["timestamp"]),"model":m["model"],"input":input,"output":output,"cacheRead":t["cached"],"cacheWrite":0,"reasoning":count("thoughts",true)})
    }).collect();
    let mut value = json!({"sessionId":meta["sessionId"],"usage":records});
    if !usage_only {
        let title = meta["summary"].as_str().map(str::to_owned).or_else(|| {
            active
                .iter()
                .find(|m| m["type"] == "user")
                .map(|m| files::text(&m["content"]).chars().take(160).collect())
        });
        value["title"] = json!(title);
        value["createdAt"] = json!(files::timestamp(&meta["startTime"]).map(|n| n * 1000));
        value["lastActiveAt"] = json!(files::timestamp(&meta["lastUpdated"]).map(|n| n * 1000));
        value["project"] = meta["directories"][0].clone();
        value["messages"] = json!(active.iter().map(message).collect::<Vec<_>>());
    }
    let events = vec![value];
    Ok((
        events,
        files::hash(&bytes),
        errors.clone(),
        errors.is_empty(),
    ))
}

fn message(m: &Value) -> Value {
    let mut blocks = Vec::new();
    for t in m["thoughts"].as_array().into_iter().flatten() {
        let text = [t["subject"].as_str(), t["description"].as_str()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n");
        blocks.push(json!({"type":"thinking","text":text}));
    }
    let content = files::text(&m["content"]);
    if !content.is_empty() {
        blocks.push(json!({"type":"text","text":content}));
    }
    for call in m["toolCalls"].as_array().into_iter().flatten() {
        let id = call["id"].as_str().unwrap_or("");
        blocks.push(transcript::call(
            id,
            call["name"].as_str().unwrap_or("tool"),
            &call["args"],
        ));
        if !call["result"].is_null() {
            blocks.push(transcript::result(
                id,
                &call["result"],
                call["status"] == "error",
            ));
        }
    }
    json!({"id":m["id"],"role":if m["type"]=="gemini" {"assistant"} else if m["type"]=="user" {"user"} else {"system"},"ts":files::timestamp(&m["timestamp"]).map(|n|n*1000),"content":content,"blocks":blocks,"meta":{"model":m["model"]}})
}

impl HarnessAdapter for Gemini {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: "gemini",
            name: "Gemini CLI",
            parser_version: "gemini-native-v1",
            default_roots: vec![dirs::home_dir()
                .unwrap_or_default()
                .join(".gemini/tmp")
                .to_string_lossy()
                .into()],
            usage: true,
            transcript: true,
            delete: false,
        }
    }
    fn discover(&self, root: &Path) -> Result<Vec<PathBuf>> {
        ensure!(root.exists(), "Gemini CLI session directory does not exist");
        if root.is_file() {
            ensure!(supported(root), "Unsupported Gemini session file");
            return Ok(vec![root.into()]);
        }
        let mut paths = Vec::new();
        for entry in walkdir::WalkDir::new(root).follow_links(false) {
            let entry = entry?;
            if entry.file_type().is_file() && supported(entry.path()) {
                // JSONL replaces the legacy snapshot with the same stem.
                if entry.path().extension().is_some_and(|e| e == "json")
                    && entry.path().with_extension("jsonl").is_file()
                {
                    continue;
                }
                paths.push(entry.into_path());
            }
        }
        paths.sort();
        Ok(paths)
    }
    fn read_source(&self, path: &Path) -> Result<(Vec<Value>, String, Vec<String>, bool)> {
        read(path, false)
    }
    fn read_usage_source(&self, path: &Path) -> Result<(Vec<Value>, String, Vec<String>, bool)> {
        read(path, true)
    }
    fn usage_reader_version(&self) -> &'static str {
        "gemini-projection-v1"
    }
    fn parse(&self, _: &Path, values: &[Value], _: &TimelineIndex) -> Result<ParseResult> {
        google_local::parse("gemini", values)
    }
    fn session(&self, path: &Path, values: &[Value]) -> Option<Session> {
        google_local::session("gemini", path, values)
    }
    fn messages(&self, values: &[Value]) -> Vec<Value> {
        google_local::messages(values)
    }
}
