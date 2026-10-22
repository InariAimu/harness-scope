use super::{files, transcript, Descriptor, HarnessAdapter, ParseResult, TimelineIndex};
use crate::model::Session;
use anyhow::Result;
use serde_json::Value;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    path::Path,
};
pub struct Codex;

/// Rollouts also store injected context as user messages. Only use the request
/// for the list preview; keep the original transcript intact.
pub(super) fn first_user_message(lines: &[Value]) -> String {
    lines
        .iter()
        .filter_map(|v| {
            if v["type"] == "response_item"
                && v["payload"]["type"] == "message"
                && v["payload"]["role"] == "user"
            {
                Some(files::text(&v["payload"]["content"]))
            } else if v["type"] == "event_msg" && v["payload"]["type"] == "user_message" {
                v["payload"]["message"].as_str().map(str::to_owned)
            } else {
                None
            }
        })
        .find_map(|text| request_text(&text).map(str::to_owned))
        .unwrap_or_default()
}

fn request_text(mut text: &str) -> Option<&str> {
    loop {
        text = text.trim();
        if text.is_empty() || text.starts_with("# AGENTS.md instructions for ") {
            return None;
        }
        let context = [
            "environment_context",
            "user_instructions",
            "external_codex_apps_open_page",
            "turn_aborted",
        ]
        .into_iter()
        .find(|tag| text.starts_with(&format!("<{tag}>")));
        if let Some(tag) = context {
            let close = format!("</{tag}>");
            let (_, rest) = text.split_once(&close)?;
            text = rest;
            continue;
        }
        if text.starts_with("# Files mentioned by the user:") {
            if let Some((_, request)) = text.split_once("\n## My request:") {
                text = request;
                continue;
            }
        }
        return Some(text);
    }
}

fn indexed_titles(root: &Path) -> HashMap<String, String> {
    // Respect the configured source, including archived logs and date subfolders,
    // rather than using CODEX_HOME for an imported source from another device.
    let root = if root.is_file() {
        root.parent().unwrap_or(root)
    } else {
        root
    };
    let home = root
        .ancestors()
        .find(|p| {
            matches!(
                p.file_name().and_then(|name| name.to_str()),
                Some("sessions" | "archived_sessions")
            )
        })
        .and_then(Path::parent)
        .unwrap_or(root);
    let mut titles = HashMap::new();
    let Ok(file) = std::fs::File::open(home.join("session_index.jsonl")) else {
        return titles;
    };
    for line in BufReader::new(file).split(b'\n') {
        let Ok(line) = line else { break };
        let Ok(entry) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        if let (Some(id), Some(title)) = (entry["id"].as_str(), entry["thread_name"].as_str()) {
            if !title.trim().is_empty() {
                // The index is append-only: a later row can rename an existing ID.
                titles.insert(id.to_owned(), title.trim().to_owned());
            }
        }
    }
    titles
}

pub fn session_id(lines: &[Value]) -> Option<&str> {
    lines
        .iter()
        .find(|v| v["type"] == "session_meta")
        .and_then(|v| v["payload"]["id"].as_str())
}
fn parent_id(meta: &Value) -> Option<&str> {
    meta["payload"]["forked_from_id"]
        .as_str()
        .or_else(|| meta["payload"]["parent_thread_id"].as_str())
        .or_else(|| {
            meta.pointer("/payload/source/subagent/thread_spawn/parent_thread_id")
                .and_then(Value::as_str)
        })
}
impl HarnessAdapter for Codex {
    fn context_id(&self, events: &[Value]) -> Option<String> {
        session_id(events).map(|id| format!("codex:{id}"))
    }
    fn context_events(&self, events: &[Value]) -> Vec<Value> {
        super::codex_usage::context(events)
    }
    fn parent_context(&self, events: &[Value]) -> Option<String> {
        let meta = events.iter().find(|v| v["type"] == "session_meta")?;
        parent_id(meta).map(|id| format!("codex:{id}"))
    }
    fn read_usage_source(&self, path: &Path) -> Result<(Vec<Value>, String, Vec<String>, bool)> {
        super::usage_read::read(path, true)
    }
    fn delete_source(&self, path: &Path) -> Result<()> {
        let sibling = if path.to_string_lossy().ends_with(".jsonl.zst") {
            Some(path.with_extension(""))
        } else {
            Some(std::path::PathBuf::from(format!("{}.zst", path.display())))
        };
        std::fs::remove_file(path)?;
        if let Some(other) = sibling.filter(|p| p.exists()) {
            std::fs::remove_file(other)?
        }
        Ok(())
    }
    fn descriptor(&self) -> Descriptor {
        let home = std::env::var_os("CODEX_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".codex"));
        Descriptor {
            id: "codex",
            name: "Codex",
            parser_version: super::codex_usage::VERSION,
            default_roots: vec![
                home.join("sessions").to_string_lossy().into(),
                home.join("archived_sessions").to_string_lossy().into(),
            ],
            usage: true,
            transcript: true,
            delete: true,
        }
    }
    fn parse(&self, path: &Path, lines: &[Value], index: &TimelineIndex) -> Result<ParseResult> {
        super::codex_usage::parse(path, lines, index)
    }
    fn read_metadata(&self, path: &Path) -> Result<Vec<Session>> {
        let lines = files::metadata_lines(path)?;
        let mut sessions = self.sessions(path, &lines);
        if let Some(modified) = std::fs::metadata(path)?
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        {
            for session in &mut sessions {
                if path.to_string_lossy().ends_with(".zst") || session.last_active_at.is_none() {
                    session.last_active_at = Some(modified.as_millis() as i64);
                }
            }
        }
        Ok(sessions)
    }
    fn session(&self, path: &Path, lines: &[Value]) -> Option<Session> {
        transcript::session("codex", path, lines)
    }
    fn enrich_metadata(&self, root: &Path, sessions: &mut [Session]) {
        if sessions.is_empty() {
            return;
        }
        let titles = indexed_titles(root);
        for session in sessions {
            if let Some(title) = titles.get(&session.session_id) {
                session.title = Some(title.clone());
            }
        }
    }
    fn messages(&self, lines: &[Value]) -> Vec<Value> {
        transcript::codex(lines)
    }
}
