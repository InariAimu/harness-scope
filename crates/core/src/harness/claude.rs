use super::{files, transcript, Descriptor, HarnessAdapter, ParseResult, TimelineIndex};
use crate::model::{ParsedUsage, Session, Tokens};
use anyhow::Result;
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};
pub struct Claude;
pub const PARSER_VERSION: &str = "claude-v2-request-timing";

fn session_id(path: &Path, v: &Value) -> String {
    v["sessionId"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into()
        })
}
impl HarnessAdapter for Claude {
    fn read_usage_source(&self, path: &Path) -> Result<(Vec<Value>, String, Vec<String>, bool)> {
        super::usage_read::read(path, false)
    }
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: "claude",
            name: "Claude Code",
            parser_version: PARSER_VERSION,
            default_roots: vec![std::env::var_os("CLAUDE_CONFIG_DIR")
                .filter(|v| !v.is_empty())
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".claude"))
                .join("projects")
                .to_string_lossy()
                .into()],
            usage: true,
            transcript: true,
            delete: true,
        }
    }
    fn parse(&self, path: &Path, lines: &[Value], _: &TimelineIndex) -> Result<ParseResult> {
        let mut out = ParseResult {
            complete: true,
            ..Default::default()
        };
        let mut messages: BTreeMap<String, ParsedUsage> = BTreeMap::new();
        let mut requests = super::claude_requests::Requests::default();
        for v in lines {
            requests.observe(&session_id(path, v), v);
        }
        for v in lines {
            if v["type"] != "assistant" {
                continue;
            }
            let m = &v["message"];
            let u = &m["usage"];
            if !u.is_object() {
                continue;
            }
            if let Err(e) = files::validate_usage(u) {
                out.complete = false;
                out.diagnostics.push(e.to_string());
                continue;
            }
            let Some(id) = m["id"].as_str() else {
                out.diagnostics
                    .push("Assistant usage has no message.id".into());
                out.complete = false;
                continue;
            };
            let Some(at) = files::timestamp(&v["timestamp"]) else {
                out.diagnostics.push(format!("{id}: missing timestamp"));
                out.complete = false;
                continue;
            };
            let session = session_id(path, v);
            let reasoning = &u["output_tokens_details"]["thinking_tokens"];
            if !reasoning.is_null() && reasoning.as_u64().is_none() {
                out.diagnostics
                    .push(format!("{id}: invalid thinking_tokens"));
                out.complete = false;
                continue;
            }
            let tokens = Tokens {
                input: files::number(u, "input_tokens"),
                output: files::number(u, "output_tokens"),
                cache_read: files::number(u, "cache_read_input_tokens"),
                cache_write: files::number(u, "cache_creation_input_tokens"),
                cache_write1h: files::number(&u["cache_creation"], "ephemeral_1h_input_tokens"),
                reasoning: reasoning.as_u64().unwrap_or(0),
            };
            if tokens.total() == 0 {
                continue;
            }
            if tokens.cache_write1h > tokens.cache_write || tokens.reasoning > tokens.output {
                out.diagnostics
                    .push(format!("{id}: invalid cache write or reasoning subset"));
                out.complete = false;
                continue;
            }
            let entry = ParsedUsage {
                request_metadata: requests.metadata(&session, id, v),
                provider: None,
                id: format!("claude:{session}:{id}"),
                session_id: session.clone(),
                native_event_id: id.into(),
                occurred_at: at,
                model: m["model"].as_str().unwrap_or("unknown").into(),
                tokens,
                reported_cost: None,
                timing: requests.timing(&session, id),
            };
            match messages.get(&entry.id) {
                Some(previous) if previous.tokens.output > entry.tokens.output => {}
                _ => {
                    messages.insert(entry.id.clone(), entry);
                }
            }
        }
        out.usage = messages.into_values().collect();
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
        let mut session = transcript::session("claude", path, lines)?;
        let prompt = lines
            .iter()
            .filter(|v| v["type"] == "user" && v["isMeta"] != true)
            .map(|v| files::text(&v["message"]["content"]))
            .find(|text| {
                let text = text.trim();
                !text.is_empty()
                    && ![
                        "<command-name>",
                        "<local-command",
                        "<bash-input>",
                        "<system-reminder>",
                    ]
                    .iter()
                    .any(|prefix| text.starts_with(prefix))
            })
            .unwrap_or_default();
        session.summary = Some(prompt.chars().take(160).collect());
        let named = |kind: &str, field: &str| {
            lines
                .iter()
                .rev()
                .filter(|v| {
                    v["type"] == kind
                        && v["sessionId"].as_str() == Some(session.session_id.as_str())
                })
                .filter_map(|v| v[field].as_str())
                .map(str::trim)
                .find(|s| !s.is_empty())
        };
        session.title = Some(
            named("custom-title", "customTitle")
                .or_else(|| named("ai-title", "aiTitle"))
                .unwrap_or_else(|| {
                    if prompt.is_empty() {
                        &session.session_id
                    } else {
                        prompt.trim()
                    }
                })
                .chars()
                .take(160)
                .collect(),
        );
        Some(session)
    }
    fn messages(&self, lines: &[Value]) -> Vec<Value> {
        transcript::claude(lines)
    }
}
