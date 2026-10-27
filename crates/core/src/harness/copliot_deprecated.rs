//! Selected VS Code chat sessions, with conservative historical usage projection.
use super::{files, Descriptor, HarnessAdapter, ParseResult, TimelineIndex};
use crate::model::{ParsedUsage, Session, Tokens};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

pub const ID: &str = "copliot-deprecated";
pub const PROVIDER: &str = "github-copliot";
pub const SELECTION_FILE: &str = ".harness-scope-copliot-deprecated.json";
pub struct CopliotDeprecated;

/// Both adapters share the same native discovery and the fixed historical boundary.
pub(super) fn discover_sessions(root: &Path, historical: bool) -> Result<Vec<PathBuf>> {
    let (old, new) = discover_both(root)?;
    Ok(if historical { old } else { new })
}

/// Discover both ownership classes together. The result is scoped to one scan;
/// newly created sessions and selection changes are observed on the next scan.
pub(crate) fn discover_both(root: &Path) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let selection: Selection = serde_json::from_slice(
        &fs::read(root.join(SELECTION_FILE))
            .context("Select historical Copilot sessions before enabling this source")?,
    )?;
    selection.validate()?;
    let mut old = Vec::new();
    let mut new = Vec::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type().is_file()
            && path
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|n| n == "chatSessions")
            && path
                .extension()
                .is_some_and(|e| e == "json" || e == "jsonl")
        {
            if let Some(id) = path.file_stem().and_then(|s| s.to_str()) {
                if selection.session_ids.contains(id) {
                    old.push(path.to_owned());
                } else {
                    new.push(path.to_owned());
                }
            }
        }
    }
    fn prefer_logs(paths: &mut Vec<PathBuf>) {
        paths.sort();
        // A mutation log supersedes a snapshot of the same session in the same directory.
        let logs: BTreeSet<_> = paths
            .iter()
            .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
            .map(|p| p.with_extension(""))
            .collect();
        paths.retain(|p| {
            p.extension().is_some_and(|e| e == "jsonl") || !logs.contains(&p.with_extension(""))
        });
    }
    prefer_logs(&mut old);
    prefer_logs(&mut new);
    Ok((old, new))
}

/// Local discovery control, not usage data or a change to native session files.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Selection {
    pub version: u32,
    pub session_ids: BTreeSet<String>,
}
impl Selection {
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.version == 1,
            "Unsupported historical Copilot selection version"
        );
        anyhow::ensure!(
            self.session_ids.iter().all(|id| !id.is_empty()
                && id.len() <= 256
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))),
            "Invalid historical Copilot session ID"
        );
        Ok(())
    }
}

fn identity(path: &Path, state: &Value) -> Result<String> {
    state["sessionId"]
        .as_str()
        .filter(|id| !id.is_empty())
        .or_else(|| path.file_stem().and_then(|s| s.to_str()))
        .map(str::to_owned)
        .context("Missing Copilot session ID")
}
fn validate_state(state: &Value) -> Result<()> {
    anyhow::ensure!(
        state["version"] == 3 && state["requests"].is_array(),
        "Unsupported VS Code chat session format"
    );
    Ok(())
}
fn milliseconds(value: &Value) -> Option<i64> {
    value.as_i64().filter(|t| *t >= 1000).map(|t| t / 1000)
}
fn key_index(key: &Value) -> Result<usize> {
    key.as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .context("Invalid Copilot array index")
}
fn child_mut<'a>(target: &'a mut Value, key: &Value) -> Result<&'a mut Value> {
    match target {
        Value::Object(object) => {
            object.get_mut(key.as_str().context("Invalid Copilot object key")?)
        }
        Value::Array(array) => array.get_mut(key_index(key)?),
        _ => None,
    }
    .context("Copilot mutation references a missing path")
}
fn replay(bytes: &[u8], snapshot: bool) -> Result<Value> {
    let text = std::str::from_utf8(bytes)?.trim_start_matches('\u{feff}');
    if snapshot {
        let state = serde_json::from_str(text)?;
        validate_state(&state)?;
        return Ok(state);
    }
    let mut state = None;
    for (line, text) in text
        .lines()
        .enumerate()
        .filter(|(_, s)| !s.trim().is_empty())
    {
        let operation: Value = serde_json::from_str(text)
            .with_context(|| format!("Invalid Copilot JSON at line {}", line + 1))?;
        let kind = operation["kind"]
            .as_u64()
            .context("Missing Copilot operation kind")?;
        if kind == 0 {
            anyhow::ensure!(state.is_none(), "Repeated Copilot initial state");
            state = Some(
                operation
                    .get("v")
                    .context("Missing Copilot initial state")?
                    .clone(),
            );
            continue;
        }
        anyhow::ensure!((1..=3).contains(&kind), "Unknown Copilot operation kind");
        let mut target = state.as_mut().context("Missing Copilot initial state")?;
        let keys = operation["k"]
            .as_array()
            .context("Missing Copilot operation path")?;
        let Some((key, parents)) = keys.split_last() else {
            continue;
        };
        for key in parents {
            target = child_mut(target, key)?;
        }
        let delete = kind == 3 || (kind == 1 && operation.get("v").is_none());
        if kind == 2 {
            if let Value::Object(object) = target {
                object
                    .entry(
                        key.as_str()
                            .context("Invalid Copilot object key")?
                            .to_owned(),
                    )
                    .or_insert(Value::Null);
            }
            let slot = child_mut(target, key)?;
            if slot.is_null() {
                *slot = json!([]);
            }
            let array = slot
                .as_array_mut()
                .context("Copilot append target is not an array")?;
            if let Some(length) = operation.get("i") {
                let length = key_index(length)?;
                anyhow::ensure!(length <= 1_000_000, "Copilot array length too large");
                array.resize(length, Value::Null);
            }
            match operation.get("v") {
                None | Some(Value::Null) => {}
                Some(Value::Array(values)) => array.extend(values.iter().cloned()),
                _ => anyhow::bail!("Invalid Copilot append values"),
            }
        } else {
            match target {
                Value::Object(object) => {
                    let key = key.as_str().context("Invalid Copilot object key")?;
                    if delete {
                        object.remove(key);
                    } else {
                        object.insert(key.into(), operation["v"].clone());
                    }
                }
                Value::Array(array) => {
                    let index = key_index(key)?;
                    anyhow::ensure!(index <= 1_000_000, "Copilot array index too large");
                    if index >= array.len() {
                        array.resize(index + 1, Value::Null);
                    }
                    array[index] = if delete {
                        Value::Null
                    } else {
                        operation["v"].clone()
                    };
                }
                _ => anyhow::bail!("Copilot mutation references a missing path"),
            }
        }
    }
    let state = state.context("Empty Copilot mutation log")?;
    validate_state(&state)?;
    Ok(state)
}
fn request_usage(sid: &str, row: &Value) -> Result<ParsedUsage> {
    let rid = row["requestId"]
        .as_str()
        .filter(|id| !id.is_empty())
        .context("Missing Copilot request ID")?;
    let occurred_at =
        milliseconds(&row["timestamp"]).context("Missing Copilot request timestamp")?;
    let metadata = &row["result"]["metadata"];
    let model = metadata["resolvedModel"]
        .as_str()
        .filter(|m| !m.trim().is_empty())
        .or_else(|| row["modelId"].as_str().filter(|m| !m.trim().is_empty()))
        .unwrap_or("unknown");
    let model = if model
        .rsplit('/')
        .next()
        .is_some_and(|m| m.eq_ignore_ascii_case("auto"))
    {
        "unknown"
    } else {
        model
    };
    let output = if row["completionTokens"].is_null() {
        0
    } else {
        row["completionTokens"]
            .as_u64()
            .filter(|n| *n <= 1_000_000_000_000)
            .context("Invalid Copilot cumulative output tokens")?
    };
    let provider = crate::providers::parse(row["providerId"].as_str())
        .or_else(|| crate::providers::parse(metadata["providerId"].as_str()))
        .filter(|id| !matches!(id.to_ascii_lowercase().as_str(), "unknown" | "auto"))
        .unwrap_or_else(|| PROVIDER.into());
    Ok(ParsedUsage {
        request_metadata: None,
        provider: Some(provider),
        id: format!("copliot-session:{sid}:{rid}"),
        session_id: sid.into(),
        native_event_id: rid.into(),
        occurred_at,
        model: model.into(),
        tokens: Tokens {
            output,
            ..Default::default()
        },
        reported_cost: None,
        timing: None,
    })
}
fn user_text(row: &Value) -> Option<&str> {
    row["message"]["text"]
        .as_str()
        .or_else(|| row["message"].as_str())
        .filter(|s| !s.trim().is_empty())
}

impl HarnessAdapter for CopliotDeprecated {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: ID,
            name: ID,
            parser_version: "copliot-deprecated-compatible-v1",
            default_roots: vec![],
            usage: true,
            transcript: true,
            delete: false,
        }
    }
    fn allows_price_estimates(&self, has_tokens: bool) -> bool {
        has_tokens
    }
    fn discover(&self, root: &Path) -> Result<Vec<PathBuf>> {
        discover_sessions(root, true)
    }
    fn read_source(&self, path: &Path) -> Result<(Vec<Value>, String, Vec<String>, bool)> {
        let bytes = fs::read(path)?;
        let state = replay(&bytes, path.extension().is_some_and(|e| e == "json"))?;
        Ok((vec![state], files::hash(&bytes), vec![], true))
    }
    fn parse(&self, path: &Path, events: &[Value], _: &TimelineIndex) -> Result<ParseResult> {
        let state = events.first().context("Missing Copilot session")?;
        validate_state(state)?;
        let sid = identity(path, state)?;
        let mut result = ParseResult {
            complete: true,
            ..Default::default()
        };
        let mut seen = BTreeSet::new();
        for row in state["requests"].as_array().unwrap() {
            if row["isSystemInitiated"] == true {
                continue;
            }
            match request_usage(&sid, row) {
                Ok(usage) if seen.insert(usage.id.clone()) => result.usage.push(usage),
                Ok(_) => {
                    result.complete = false;
                    result
                        .diagnostics
                        .push("Duplicate Copilot request identity".into());
                }
                Err(error) => {
                    result.complete = false;
                    result
                        .diagnostics
                        .push(format!("{}: {error}", path.display()));
                }
            }
        }
        Ok(result)
    }
    fn session(&self, path: &Path, events: &[Value]) -> Option<Session> {
        let state = events.first()?;
        validate_state(state).ok()?;
        let sid = identity(path, state).ok()?;
        let rows = state["requests"].as_array()?;
        let first = rows
            .iter()
            .filter(|r| r["isSystemInitiated"] != true)
            .find_map(user_text);
        let created = milliseconds(&state["creationDate"]).or_else(|| {
            rows.iter()
                .filter_map(|r| milliseconds(&r["timestamp"]))
                .min()
        });
        let last = rows
            .iter()
            .flat_map(|r| {
                [
                    milliseconds(&r["timestamp"]),
                    milliseconds(&r["modelState"]["completedAt"]),
                ]
            })
            .flatten()
            .chain(milliseconds(&state["lastMessageDate"]))
            .max()
            .or(created);
        let workspace = path.parent()?.parent()?.join("workspace.json");
        let project_dir = fs::read(workspace)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|v| {
                reqwest::Url::parse(v["folder"].as_str()?)
                    .ok()?
                    .to_file_path()
                    .ok()
            })
            .map(|p| p.to_string_lossy().into_owned());
        Some(Session {
            provider_id: ID.into(),
            session_id: sid,
            title: Some(
                state["customTitle"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .or(first)
                    .unwrap_or("unknown")
                    .chars()
                    .take(160)
                    .collect(),
            ),
            summary: first.map(|s| s.chars().take(160).collect()),
            project_dir,
            created_at: created,
            last_active_at: last,
            source_path: Some(path.to_string_lossy().into_owned()),
            resume_command: None,
            can_delete: false,
        })
    }
    fn messages(&self, events: &[Value]) -> Vec<Value> {
        let Some(rows) = events.first().and_then(|s| s["requests"].as_array()) else {
            return vec![];
        };
        let mut messages = Vec::new();
        for row in rows {
            if row["isSystemInitiated"] == true {
                continue;
            }
            let rid = row["requestId"].as_str().unwrap_or("unknown");
            let ts = row["timestamp"].as_i64();
            if let Some(text) = user_text(row) {
                messages.push(json!({"role":"user","id":format!("{rid}:user"),"content":text,"ts":ts,"blocks":[{"type":"text","text":text}]}));
            }
            let response = row["response"]
                .as_array()
                .map(|parts| {
                    parts
                        .iter()
                        .filter_map(|p| p["value"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .filter(|s| !s.trim().is_empty())
                .or_else(|| row["response"].as_str().map(str::to_owned));
            if let Some(text) = response {
                messages.push(json!({"role":"assistant","id":format!("{rid}:assistant"),"content":text,"ts":row["modelState"]["completedAt"].as_i64(),"blocks":[{"type":"text","text":text}]}));
            }
        }
        messages
    }
}
