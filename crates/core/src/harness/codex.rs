use super::{files, transcript, Descriptor, HarnessAdapter, ParseResult, TimelineIndex};
use crate::model::{ParsedUsage, Session, Tokens};
use anyhow::Result;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
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
fn signature(v: &Value) -> String {
    format!("{}|{}", v["timestamp"], v["payload"]["info"])
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
fn is_usage(v: &Value) -> bool {
    v["type"] == "event_msg"
        && v["payload"]["type"] == "token_count"
        && v["payload"]["info"].is_object()
}
fn counts(v: &Value) -> [u64; 5] {
    [
        files::number(v, "input_tokens"),
        files::number(v, "output_tokens"),
        files::number(v, "cached_input_tokens"),
        v.get("cache_write_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| files::number(v, "cache_creation_input_tokens")),
        files::number(v, "reasoning_output_tokens"),
    ]
}
impl HarnessAdapter for Codex {
    fn context_id(&self, events: &[Value]) -> Option<String> {
        session_id(events).map(|id| format!("codex:{id}"))
    }
    fn context_events(&self, events: &[Value]) -> Vec<Value> {
        events.iter().filter(|v|is_usage(v)).map(|v|serde_json::json!({"type":"event_msg","timestamp":v["timestamp"],"payload":{"type":"token_count","info":v["payload"]["info"]}})).collect()
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
            parser_version: "codex-v4-provider",
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
        let mut out = ParseResult {
            complete: true,
            ..Default::default()
        };
        let Some(session) = session_id(lines) else {
            anyhow::bail!("{}: missing session_meta.id", path.display())
        };
        let meta = lines.iter().find(|v| v["type"] == "session_meta").unwrap();
        let parent = parent_id(meta);
        let mut inherited = HashSet::new();
        let mut baseline: Option<[u64; 5]> = None;
        if let Some(parent) = parent {
            let Some(parent_lines) = index.get(&format!("codex:{parent}")) else {
                out.complete = false;
                out.diagnostics.push(format!(
                    "{session}: parent {parent} is missing; usage deferred"
                ));
                return Ok(out);
            };
            let cutoff = files::timestamp(&meta["timestamp"]);
            for v in parent_lines.iter().filter(|v| is_usage(v)) {
                if cutoff
                    .is_some_and(|at| files::timestamp(&v["timestamp"]).is_some_and(|t| t > at))
                {
                    continue;
                }
                inherited.insert(signature(v));
                if v["payload"]["info"]["total_token_usage"].is_object() {
                    baseline = Some(counts(&v["payload"]["info"]["total_token_usage"]));
                }
            }
        }
        let mut model = "unknown".to_owned();
        let mut provider = crate::providers::parse(meta["payload"]["model_provider"].as_str());
        let mut seen = HashSet::new();
        let mut ordinal = 0;
        let mut timer = super::codex_timing::RequestTimer::default();
        for v in lines {
            timer.observe(v);
            if v["type"] == "turn_context" || v["type"] == "session_meta" {
                if let Some(id) = crate::providers::parse(v["payload"]["model_provider"].as_str()) {
                    provider = Some(id);
                }
                if let Some(m) = v["payload"]["model"].as_str() {
                    model = m.into()
                }
            }
            if !is_usage(v) {
                continue;
            }
            ordinal += 1;
            let sig = signature(v);
            if !seen.insert(sig.clone()) {
                continue;
            }
            let info = &v["payload"]["info"];
            if let Some(invalid) = ["total_token_usage", "last_token_usage"]
                .iter()
                .filter(|key| info[**key].is_object())
                .find_map(|key| files::validate_usage(&info[*key]).err())
            {
                out.complete = false;
                out.diagnostics.push(invalid.to_string());
                continue;
            }
            let total = info["total_token_usage"]
                .is_object()
                .then(|| counts(&info["total_token_usage"]));
            if inherited.contains(&sig) {
                continue;
            }
            // Repeated cumulative snapshots are observations, not additional calls.
            if total.is_some() && total == baseline {
                continue;
            }
            let delta = if info["last_token_usage"].is_object() {
                counts(&info["last_token_usage"])
            } else if let Some(current) = total {
                match baseline {
                    Some(previous) => {
                        if current[0] < previous[0] || current[1] < previous[1] {
                            out.diagnostics.push(format!("{session}:{ordinal}: cumulative counter reset without last_token_usage; deferred"));
                            out.complete = false;
                            baseline = Some(current);
                            continue;
                        }
                        std::array::from_fn(|i| current[i].saturating_sub(previous[i]))
                    }
                    None => current,
                }
            } else {
                out.complete = false;
                out.diagnostics
                    .push(format!("{session}:{ordinal}: unknown usage fields"));
                continue;
            };
            if let Some(total) = total {
                baseline = Some(total)
            }
            let Some(at) = files::timestamp(&v["timestamp"]) else {
                out.complete = false;
                out.diagnostics
                    .push(format!("{session}:{ordinal}: missing timestamp"));
                continue;
            };
            if delta[2] + delta[3] > delta[0] || delta[4] > delta[1] {
                out.complete = false;
                out.diagnostics.push(format!(
                    "{session}:{ordinal}: invalid cache/reasoning subset"
                ));
                continue;
            }
            let tokens = Tokens {
                input: delta[0] - delta[2] - delta[3],
                output: delta[1],
                cache_read: delta[2],
                cache_write: delta[3],
                cache_write1h: 0,
                reasoning: delta[4],
            };
            if tokens.total() == 0 {
                continue;
            }
            out.usage.push(ParsedUsage {
                provider: provider.clone(),
                id: format!("codex:{session}:token:{ordinal}"),
                session_id: session.into(),
                native_event_id: format!("token:{ordinal}"),
                occurred_at: at,
                model: model.clone(),
                tokens,
                reported_cost: None,
                timing: timer.finish(v, delta),
            });
        }
        Ok(out)
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
