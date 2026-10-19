use super::{files, transcript, Descriptor, HarnessAdapter, ParseResult, TimelineIndex};
use crate::model::{ParsedUsage, Session, Tokens};
use anyhow::Result;
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};
pub struct Claude;
impl HarnessAdapter for Claude {
    fn read_usage_source(&self, path: &Path) -> Result<(Vec<Value>, String, Vec<String>, bool)> {
        super::usage_read::read(path, false)
    }
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: "claude",
            name: "Claude Code",
            parser_version: "claude-v1",
            default_roots: vec![dirs::home_dir()
                .unwrap_or_default()
                .join(".claude/projects")
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
            let session = v["sessionId"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    path.file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into()
                });
            let tokens = Tokens {
                input: files::number(u, "input_tokens"),
                output: files::number(u, "output_tokens"),
                cache_read: files::number(u, "cache_read_input_tokens"),
                cache_write: files::number(u, "cache_creation_input_tokens"),
                cache_write1h: files::number(&u["cache_creation"], "ephemeral_1h_input_tokens"),
                reasoning: 0,
            };
            if tokens.total() == 0 {
                continue;
            }
            if tokens.cache_write1h > tokens.cache_write {
                out.diagnostics
                    .push(format!("{id}: invalid cache write subset"));
                out.complete = false;
                continue;
            }
            let entry = ParsedUsage {
                provider: None,
                id: format!("claude:{session}:{id}"),
                session_id: session,
                native_event_id: id.into(),
                occurred_at: at,
                model: m["model"].as_str().unwrap_or("unknown").into(),
                tokens,
                reported_cost: None,
                timing: None,
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
        transcript::session("claude", path, lines)
    }
    fn messages(&self, lines: &[Value]) -> Vec<Value> {
        transcript::claude(lines)
    }
}
