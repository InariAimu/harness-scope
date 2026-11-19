//! Shared projection for native Google clients. Provider ownership is not inferred
//! from a model name or the machine's current authentication settings.
use super::{files, ParseResult};
use crate::model::{ParsedUsage, Session, Tokens, UsageTiming};
use anyhow::{ensure, Context, Result};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};

pub(super) fn fingerprint(events: &[Value]) -> Result<String> {
    Ok(files::hash(&serde_json::to_vec(events)?))
}

pub(super) fn parse(harness: &str, values: &[Value]) -> Result<ParseResult> {
    let value = values.first().context("Missing native conversation")?;
    let session = value["sessionId"].as_str().context("Missing session ID")?;
    let mut result = ParseResult {
        complete: true,
        ..Default::default()
    };
    let mut records = BTreeMap::<String, ParsedUsage>::new();
    let mut conflicts = HashSet::new();
    for (index, item) in value["usage"]
        .as_array()
        .context("Missing usage projection")?
        .iter()
        .enumerate()
    {
        let parsed = (|| -> Result<ParsedUsage> {
            let native = item["id"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control))
                .context("Missing generation identity")?;
            let origin = item["originSession"].as_str().unwrap_or(session);
            ensure!(
                !origin.is_empty() && origin.len() <= 512 && !origin.chars().any(char::is_control),
                "Invalid session identity"
            );
            let model = item["model"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control))
                .context("Missing native model name")?;
            let at = item["at"]
                .as_i64()
                .filter(|t| *t > 0 && chrono::DateTime::from_timestamp(*t, 0).is_some())
                .context("Missing native generation time")?;
            let count = |key: &str| {
                item[key]
                    .as_u64()
                    .filter(|v| *v <= 1_000_000_000_000)
                    .with_context(|| format!("Missing or invalid {key} tokens"))
            };
            let tokens = Tokens {
                input: count("input")?,
                output: count("output")?,
                cache_read: count("cacheRead")?,
                cache_write: count("cacheWrite")?,
                reasoning: count("reasoning")?,
                ..Default::default()
            };
            ensure!(
                tokens.reasoning <= tokens.output && tokens.total() <= 1_000_000_000_000,
                "Invalid token total or reasoning subset"
            );
            Ok(ParsedUsage {
                id: format!(
                    "{harness}:{}",
                    files::hash(&serde_json::to_vec(&(origin, native))?)
                ),
                native_event_id: native.into(),
                session_id: origin.into(),
                occurred_at: at,
                model: model.into(),
                tokens,
                provider: None,
                model_usage: None,
                request_metadata: None,
                reported_cost: None,
                timing: if harness == "antigravity" {
                    serde_json::from_value::<UsageTiming>(item["timing"].clone())
                        .ok()
                        .filter(|t| {
                            (100..=3_600_000).contains(&t.duration_ms)
                                && t.first_token_ms
                                    .is_some_and(|v| (0..=t.duration_ms).contains(&v))
                                && t.duration_source == "antigravity_chat_model"
                                && t.first_token_source.as_deref() == Some("antigravity_chat_model")
                        })
                } else {
                    None
                },
            })
        })();
        match parsed {
            Ok(record) => {
                if conflicts.contains(&record.id) {
                    continue;
                }
                if let Some(old) = records.get(&record.id) {
                    if old.tokens != record.tokens
                        || old.model != record.model
                        || old.occurred_at != record.occurred_at
                        || old.timing != record.timing
                    {
                        conflicts.insert(record.id.clone());
                        records.remove(&record.id);
                        result.complete = false;
                        result.diagnostics.push(format!(
                            "{harness}: conflicting generation identity at record {}",
                            index + 1
                        ));
                    }
                } else {
                    records.insert(record.id.clone(), record);
                }
            }
            Err(error) => {
                result.complete = false;
                result.diagnostics.push(format!(
                    "{harness}: deferred generation {}: {error}",
                    index + 1
                ));
            }
        }
    }
    result.usage = records.into_values().collect();
    Ok(result)
}

pub(super) fn session(harness: &str, path: &Path, values: &[Value]) -> Option<Session> {
    let v = values.first()?;
    Some(Session {
        provider_id: harness.into(),
        session_id: v["sessionId"].as_str()?.into(),
        title: v["title"].as_str().map(str::to_owned),
        summary: None,
        project_dir: v["project"].as_str().map(str::to_owned),
        created_at: v["createdAt"].as_i64(),
        last_active_at: v["lastActiveAt"].as_i64(),
        source_path: Some(path.to_string_lossy().into()),
        resume_command: None,
        can_delete: false,
    })
}

pub(super) fn messages(values: &[Value]) -> Vec<Value> {
    values
        .first()
        .and_then(|v| v["messages"].as_array())
        .cloned()
        .unwrap_or_default()
}
