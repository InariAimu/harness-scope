//! New native VS Code Copilot sessions, projected onto the existing usage contract.
use super::{
    copliot_deprecated::{discover_sessions, CopliotDeprecated},
    Descriptor, HarnessAdapter, ParseResult, TimelineIndex,
};
use crate::model::Session;
use anyhow::Result;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const ID: &str = "copliot";
pub struct Copliot;

fn extension(value: &Value) -> Option<&str> {
    value.as_str().or_else(|| value["value"].as_str())
}
fn copilot_extension(id: &str) -> bool {
    id.eq_ignore_ascii_case("github.copilot-chat") || id.eq_ignore_ascii_case("github.copilot")
}
fn copilot_request(row: &Value) -> bool {
    // An explicit foreign agent takes precedence over a selected Copilot model.
    if let Some(id) = extension(&row["agent"]["extensionId"]) {
        return copilot_extension(id);
    }
    row["agent"]["id"]
        .as_str()
        .is_some_and(|id| id.starts_with("github.copilot."))
        || row["modelId"]
            .as_str()
            .is_some_and(|id| id.starts_with("copilot/"))
}
fn copilot_events(events: &[Value]) -> Vec<Value> {
    let mut events = events.to_vec();
    if let Some(rows) = events
        .first_mut()
        .and_then(|s| s["requests"].as_array_mut())
    {
        rows.retain(copilot_request);
    }
    events
}

impl HarnessAdapter for Copliot {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: ID,
            name: "Github Copliot",
            parser_version: "copliot-vscode-compatible-v1",
            // An explicit historical boundary is required before enabling a User root.
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
        discover_sessions(root, false)
    }
    fn read_source(&self, path: &Path) -> Result<(Vec<Value>, String, Vec<String>, bool)> {
        let data = CopliotDeprecated.read_source(path)?;
        // Discovery uses the filename for ownership. Never let an inconsistent native
        // ID bypass the historical boundary or overwrite a historical fact.
        if let Some(id) = data.0.first().and_then(|s| s["sessionId"].as_str()) {
            anyhow::ensure!(
                path.file_stem().and_then(|s| s.to_str()) == Some(id),
                "Copilot session ID differs from its filename"
            );
        }
        Ok(data)
    }
    fn parse(&self, path: &Path, events: &[Value], index: &TimelineIndex) -> Result<ParseResult> {
        // The compatible profile intentionally retains only cumulative output.
        // promptTokens is a context/last-call observation, not uncached turn input;
        // Credits are not USD. A request remains one fact even with multiple calls.
        let events = copilot_events(events);
        let mut parsed = CopliotDeprecated.parse(path, &events, index)?;
        let mut rows = std::collections::HashMap::new();
        for row in events[0]["requests"].as_array().unwrap() {
            if let Some(id) = row["requestId"].as_str() {
                // The shared parser retains the first occurrence and diagnoses duplicates.
                rows.entry(id).or_insert(row);
            }
        }
        let mut diagnostics = Vec::new();
        parsed.usage.retain_mut(|usage| {
            let row = rows[usage.native_event_id.as_str()];
            // Whole-turn multi-model output cannot be priced as the last model.
            // Until the richer per-model contract is available, keep the sum under unknown.
            if let Some(totals) = row.get("modelTotals").filter(|v| !v.is_null()) {
                let aggregate = (|| -> Result<(String, u64)> {
                    let totals = totals
                        .as_array()
                        .filter(|v| !v.is_empty())
                        .ok_or_else(|| anyhow::anyhow!("Invalid Copilot model totals"))?;
                    let mut models = std::collections::BTreeSet::new();
                    let mut output = 0u64;
                    for total in totals {
                        let model = total["model"]
                            .as_str()
                            .filter(|s| !s.trim().is_empty())
                            .ok_or_else(|| anyhow::anyhow!("Missing Copilot totals model"))?;
                        models.insert(model);
                        let count = total["outputTokens"]
                            .as_u64()
                            .filter(|n| *n <= 1_000_000_000_000)
                            .ok_or_else(|| anyhow::anyhow!("Invalid Copilot totals output"))?;
                        output = output
                            .checked_add(count)
                            .filter(|n| *n <= 1_000_000_000_000)
                            .ok_or_else(|| anyhow::anyhow!("Copilot totals output overflow"))?;
                    }
                    let model = if models.len() == 1 {
                        *models.first().unwrap()
                    } else {
                        "unknown"
                    };
                    let model = if model
                        .rsplit('/')
                        .next()
                        .is_some_and(|m| m.eq_ignore_ascii_case("auto"))
                    {
                        "unknown"
                    } else {
                        model
                    };
                    Ok((model.into(), output))
                })();
                match aggregate {
                    Ok((model, output)) => {
                        usage.model = model;
                        usage.tokens.output = output;
                    }
                    Err(error) => {
                        diagnostics.push(format!("{}: {error}", path.display()));
                        return false;
                    }
                }
            }
            true
        });
        parsed.complete &= diagnostics.is_empty();
        parsed.diagnostics.extend(diagnostics);
        Ok(parsed)
    }
    fn session(&self, path: &Path, events: &[Value]) -> Option<Session> {
        let state = events.first()?;
        let selected = &state["inputState"]["selectedModel"];
        let has_copilot = state["requests"].as_array()?.iter().any(copilot_request)
            || extension(&selected["metadata"]["extension"]).is_some_and(copilot_extension)
            || selected["identifier"]
                .as_str()
                .is_some_and(|id| id.starts_with("copilot/"));
        if !has_copilot {
            return None;
        }
        let mut session = CopliotDeprecated.session(path, &copilot_events(events))?;
        session.provider_id = ID.into();
        Some(session)
    }
    fn messages(&self, events: &[Value]) -> Vec<Value> {
        CopliotDeprecated.messages(&copilot_events(events))
    }
}
