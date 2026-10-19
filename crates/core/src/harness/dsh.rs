//! DeepSeek Harness session logs: disjoint per-call usage, never stream totals.
use super::{files, transcript, Descriptor, HarnessAdapter, ParseResult, TimelineIndex};
use crate::model::{ParsedUsage, Session, Tokens, UsageTiming};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub struct Dsh;

fn home() -> PathBuf {
    std::env::var_os("DSH_HOME")
        .filter(|v| !v.to_string_lossy().trim().is_empty())
        .map(|v| {
            let value = v.to_string_lossy();
            if value == "~" {
                dirs::home_dir().unwrap_or_default()
            } else if value.starts_with("~/") || value.starts_with("~\\") {
                dirs::home_dir().unwrap_or_default().join(&value[2..])
            } else {
                PathBuf::from(v)
            }
        })
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".dsh"))
}

fn header(events: &[Value]) -> Result<&Value> {
    let h = events.first().context("Missing DSH session header")?;
    anyhow::ensure!(h["type"] == "session", "Missing DSH session header");
    anyhow::ensure!(
        matches!(h["version"].as_u64(), Some(0 | 4)),
        "Unsupported DSH session format"
    );
    anyhow::ensure!(
        h["id"].as_str().is_some_and(|s| !s.is_empty()),
        "Missing DSH session ID"
    );
    anyhow::ensure!(
        h["createdAt"].as_i64().is_some_and(|t| t > 0),
        "Missing DSH session timestamp"
    );
    Ok(h)
}

fn artifact_version(path: &Path) -> Option<u64> {
    let name = path.file_name()?.to_str()?;
    let stem = name
        .strip_suffix(".jsonl.zstd")
        .or_else(|| name.strip_suffix(".jsonl.zst"))
        .or_else(|| name.strip_suffix(".jsonl"))?;
    if stem == "session" {
        Some(0)
    } else {
        let digits = stem.strip_prefix("session.v")?;
        if digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        digits.parse().ok().filter(|n| *n <= 9_007_199_254_740_991)
    }
}

fn counter(usage: &Value, name: &str, required: bool) -> Result<u64> {
    let Some(value) = usage.get(name) else {
        anyhow::ensure!(!required, "Missing DSH {name}");
        return Ok(0);
    };
    value
        .as_u64()
        .filter(|n| *n <= 1_000_000_000_000)
        .with_context(|| format!("Invalid DSH {name}"))
}

/// Packed rows consume multiple event sequence numbers but contain no billable usage.
fn sequence_span(event: &Value) -> Result<(u64, u64)> {
    if matches!(
        event["type"].as_str(),
        Some("text-chunks" | "reasoning-chunks" | "tool-call-chunks")
    ) {
        let d = &event["data"];
        let values = if event["type"] == "tool-call-chunks" {
            &d["args"]
        } else {
            &d["texts"]
        };
        let values = values.as_array().context("Invalid DSH packed chunks")?;
        let gaps = d["dt"].as_array().context("Missing DSH chunk time gaps")?;
        anyhow::ensure!(
            !values.is_empty()
                && values.iter().all(Value::is_string)
                && gaps.len() == values.len() - 1
                && gaps.iter().all(|v| v.as_i64().is_some()),
            "Invalid DSH packed chunk run"
        );
        let seq = event["seq0"]
            .as_u64()
            .context("Missing DSH packed sequence")?;
        Ok((seq, values.len() as u64))
    } else {
        Ok((
            event["seq"]
                .as_u64()
                .context("Missing DSH event sequence")?,
            1,
        ))
    }
}

fn delta_time(event: &Value) -> Option<i64> {
    match event["type"].as_str()? {
        "assistant/chunk" | "chunk" => {
            let chunk = if event["type"] == "chunk" {
                &event["chunk"]
            } else {
                &event["data"]["chunk"]
            };
            if chunk["type"] == "tool-call-delta" && chunk["name"].is_string() {
                return event["time"].as_i64();
            }
            let text = match chunk["type"].as_str()? {
                "text-delta" | "reasoning-delta" => chunk["text"].as_str(),
                "tool-call-delta" => chunk["argumentsDelta"].as_str(),
                _ => None,
            }?;
            (!text.is_empty()).then(|| event["time"].as_i64()).flatten()
        }
        "text-chunks" | "reasoning-chunks" | "tool-call-chunks" => {
            // V0 storage rows and V4 nested stream rows share the run encoding.
            let d = event.get("data").unwrap_or(event);
            let values = if event["type"] == "tool-call-chunks" {
                &d["args"]
            } else {
                &d["texts"]
            };
            let values = values.as_array()?;
            let mut time = event["time0"].as_i64()?;
            if event["type"] == "tool-call-chunks" && d["name"].is_string() {
                return Some(time);
            }
            for (i, value) in values.iter().enumerate() {
                if i > 0 {
                    time = time.checked_add(d["dt"][i - 1].as_i64()?)?;
                }
                if value.as_str().is_some_and(|text| !text.is_empty()) {
                    return Some(time);
                }
            }
            None
        }
        _ => None,
    }
}

fn seed_boundary(h: &Value, events: &[Value]) -> Result<u64> {
    if h["version"] == 4 {
        anyhow::ensure!(
            h.get("seedLength").is_none(),
            "Retired DSH v4 seedLength header"
        );
        let seeded = h["isSeeded"]
            .as_bool()
            .context("Missing DSH v4 isSeeded header")?;
        // Nested forks retain older markers. Only the last inherited marker owns this cut.
        let marker = events.iter().rev().find(|event| {
            event["type"] == "session/end-seed" && event["data"]["inherited"] == true
        });
        return match (seeded, marker) {
            (true, Some(event)) => event["seq"]
                .as_u64()
                .context("Invalid DSH inherited marker"),
            (false, None) => Ok(0),
            _ => anyhow::bail!("DSH v4 header disagrees with inherited end-seed marker"),
        };
    }
    if let Some(value) = h.get("seedLength") {
        return value
            .as_u64()
            .context("Invalid DSH inherited event boundary");
    }
    // end-seed also marks ordinary resumes, so it cannot establish fork lineage.
    anyhow::ensure!(
        h["isSeeded"] != true,
        "Missing DSH inherited event boundary"
    );
    anyhow::ensure!(
        h["parentSession"].is_null() || h["origin"] == "subagent",
        "Missing DSH inherited event boundary"
    );
    Ok(0)
}

fn parse_call(
    event: &Value,
    session: &str,
    timing: Option<UsageTiming>,
) -> Result<Option<ParsedUsage>> {
    let d = &event["data"];
    let usage = &d["usage"];
    // Replacement nodes replay history; an empty usage-host message is still billable.
    if event["surfaceOp"].is_object() {
        return Ok(None);
    }
    anyhow::ensure!(usage.is_object(), "Missing or invalid DSH usage");
    let tokens = Tokens {
        input: counter(usage, "inputTokens", true)?,
        output: counter(usage, "outputTokens", true)?,
        cache_read: counter(usage, "cacheReadTokens", false)?,
        cache_write: counter(usage, "cacheWriteTokens", false)?,
        reasoning: counter(usage, "reasoningTokens", false)?,
        cache_write1h: 0,
    };
    anyhow::ensure!(
        tokens.reasoning <= tokens.output && tokens.total() <= 1_000_000_000_000,
        "Invalid DSH token subsets or total"
    );
    if tokens.total() == 0 {
        return Ok(None);
    }
    let message = &d["message"];
    let id = message["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .context("Missing DSH message ID")?;
    let model = message["source"]["model"]
        .as_str()
        .filter(|s| !s.is_empty())
        .context("Missing DSH model")?;
    let at = event["time"]
        .as_i64()
        .filter(|t| *t > 0)
        .context("Missing DSH request timestamp")?;
    Ok(Some(ParsedUsage {
        provider: crate::providers::parse(message["source"]["provider"].as_str()),
        id: format!("dsh:{session}:{id}"),
        session_id: session.into(),
        native_event_id: id.into(),
        occurred_at: at / 1000,
        model: model.into(),
        tokens,
        reported_cost: None,
        timing,
    }))
}

impl HarnessAdapter for Dsh {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: "dsh",
            name: "DeepSeek Harness",
            parser_version: "dsh-jsonl-v2-provider",
            default_roots: vec![home().join("sessions").to_string_lossy().into()],
            usage: true,
            transcript: true,
            delete: false,
        }
    }

    fn discover(&self, root: &Path) -> Result<Vec<PathBuf>> {
        anyhow::ensure!(
            root.exists(),
            "DSH source does not exist: {}",
            root.display()
        );
        if root.is_file() {
            anyhow::ensure!(
                artifact_version(root).is_some(),
                "Unsupported DSH session filename"
            );
            return Ok(vec![root.to_owned()]);
        }
        let sessions = root.join("sessions");
        let root = if sessions.is_dir() {
            sessions.as_path()
        } else {
            root
        };
        let mut selected: BTreeMap<PathBuf, (u64, PathBuf)> = BTreeMap::new();
        for entry in walkdir::WalkDir::new(root).follow_links(false) {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let Some(version) = artifact_version(entry.path()) else {
                continue;
            };
            let dir = entry.path().parent().unwrap_or(root).to_owned();
            match selected.get(&dir) {
                Some((old, _)) if *old > version => continue,
                Some((old, _)) if *old == version => anyhow::bail!(
                    "Multiple DSH encodings for session directory: {}",
                    dir.display()
                ),
                _ => {
                    selected.insert(dir, (version, entry.into_path()));
                }
            }
        }
        Ok(selected.into_values().map(|(_, path)| path).collect())
    }

    fn parse(&self, path: &Path, events: &[Value], _: &TimelineIndex) -> Result<ParseResult> {
        let h = header(events)?;
        anyhow::ensure!(
            artifact_version(path) == h["version"].as_u64(),
            "DSH filename/header version mismatch"
        );
        let session = h["id"].as_str().unwrap();
        let boundary = seed_boundary(h, events)?;
        let mut out = ParseResult {
            complete: true,
            ..Default::default()
        };
        let mut expected = 0;
        let mut calls = BTreeMap::new();
        let mut steps: BTreeMap<(u64, u64), (i64, Option<i64>)> = BTreeMap::new();
        for event in events.iter().skip(1) {
            let span = sequence_span(event).and_then(|(seq, len)| {
                anyhow::ensure!(
                    seq == expected,
                    "DSH event sequence gap: expected {expected}, got {seq}"
                );
                Ok((seq, seq.checked_add(len).context("DSH sequence overflow")?))
            });
            let (seq, next) = match span {
                Ok(span) => span,
                Err(e) => {
                    out.complete = false;
                    out.diagnostics.push(e.to_string());
                    break;
                }
            };
            expected = next;
            if seq < boundary {
                continue;
            }
            let d = &event["data"];
            let key = (
                d["turn"].as_u64().unwrap_or(0),
                d["step"].as_u64().unwrap_or(0),
            );
            if event["type"] == "step/start" {
                if let Some(time) = event["time"].as_i64() {
                    steps.insert(key, (time, None));
                }
            }
            if let (Some((_, first)), Some(time)) = (steps.get_mut(&key), delta_time(event)) {
                if first.is_none() {
                    *first = Some(time);
                }
            }
            if event["type"] == "assistant/attempt" {
                if let Some((_, first)) = steps.get_mut(&key) {
                    if first.is_none() {
                        *first = d["stream"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .find_map(delta_time);
                    }
                }
            }
            if event["type"] != "assistant/message" {
                continue;
            }
            let timing = steps.get(&key).and_then(|(start, first)| {
                let nested = d["stream"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find_map(delta_time);
                UsageTiming::estimate(
                    *start,
                    event["time"].as_i64()?,
                    first.or(nested),
                    "dsh_step_message",
                    "dsh_stream_delta",
                )
            });
            match parse_call(event, session, timing) {
                Ok(Some(call)) => {
                    calls.insert(call.id.clone(), call);
                }
                Ok(None) => {}
                Err(e) => {
                    out.complete = false;
                    out.diagnostics.push(format!("DSH seq {seq}: {e}"));
                }
            }
        }
        if expected < boundary {
            out.complete = false;
            out.diagnostics
                .push("DSH inherited prefix is incomplete".into());
        }
        out.usage = calls.into_values().collect();
        Ok(out)
    }

    fn session(&self, path: &Path, events: &[Value]) -> Option<Session> {
        let h = header(events).ok()?;
        let summary = events
            .iter()
            .find(|e| {
                e["type"] == "user/message"
                    && e["surfaceOp"] == "append"
                    && e["data"]["source"]["kind"] == "user"
            })
            .map(|e| {
                files::text(&e["data"]["content"])
                    .chars()
                    .take(160)
                    .collect::<String>()
            });
        let title = events
            .iter()
            .rev()
            .find(|e| e["type"] == "session/title")
            .and_then(|e| e["data"]["title"].as_str())
            .map(str::to_owned)
            .or_else(|| summary.clone());
        Some(Session {
            provider_id: "dsh".into(),
            session_id: h["id"].as_str()?.into(),
            title,
            summary,
            project_dir: h["cwd"].as_str().map(str::to_owned),
            created_at: h["createdAt"].as_i64(),
            last_active_at: events
                .iter()
                .filter_map(|e| e["time"].as_i64())
                .max()
                .or(h["createdAt"].as_i64()),
            source_path: Some(path.to_string_lossy().into()),
            resume_command: None,
            can_delete: false,
        })
    }

    fn messages(&self, events: &[Value]) -> Vec<Value> {
        let mut out = Vec::new();
        for event in events {
            if event["surfaceOp"].is_object() {
                continue;
            }
            let d = &event["data"];
            let (role, message) = match event["type"].as_str() {
                Some("user/message") if event["surfaceOp"] == "append" => ("user", d),
                Some("assistant/message") => ("assistant", &d["message"]),
                Some("tool/result") => ("tool", &d["message"]),
                _ => continue,
            };
            let mut blocks = Vec::new();
            if role == "tool" {
                let call_id = message["source"]["callId"]
                    .as_str()
                    .or_else(|| message["toolCallId"].as_str())
                    .unwrap_or("");
                let nested = message["content"]
                    .as_array()
                    .and_then(|xs| xs.iter().find(|b| b["type"] == "tool-result"));
                let content = nested.map(|b| &b["content"]).unwrap_or(&message["content"]);
                let error =
                    message["isError"] == true || nested.is_some_and(|b| b["isError"] == true);
                let mut result = transcript::result(call_id, content, error);
                result["status"] = json!(if error { "error" } else { "success" });
                blocks.push(result);
            } else {
                for block in message["content"].as_array().into_iter().flatten() {
                    match block["type"].as_str() {
                        Some("text") => blocks.push(json!({"type":"text","text":block["text"]})),
                        Some("reasoning") => {
                            blocks.push(json!({"type":"thinking","text":block["text"]}))
                        }
                        Some("tool-call") => blocks.push(transcript::call(
                            block["id"].as_str().unwrap_or(""),
                            block["name"].as_str().unwrap_or("tool"),
                            &block["arguments"],
                        )),
                        _ => {}
                    }
                }
            }
            if blocks.is_empty() {
                continue;
            }
            let injected = role == "user"
                && message["source"]["kind"]
                    .as_str()
                    .is_some_and(|kind| kind != "user");
            out.push(json!({"id":message["id"],"role":role,"injected":injected,"ts":event["time"],"content":files::text(&message["content"]),"blocks":blocks,
                "meta":{"model":message["source"]["model"],"inputTokens":d["usage"]["inputTokens"],"outputTokens":d["usage"]["outputTokens"]}}));
        }
        out
    }
}
