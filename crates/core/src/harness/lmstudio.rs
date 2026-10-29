//! LM Studio 0.4.x desktop conversations only. Never reads server/API history.
use super::{files, Descriptor, HarnessAdapter, ParseResult, TimelineIndex};
use crate::model::{ParsedUsage, Session, Tokens, UsageTiming};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};

pub struct LmStudio;
const SUFFIX: &str = ".conversation.json";
const STATS: &[&str] = &[
    "promptTokensCount",
    "predictedTokensCount",
    "totalTokensCount",
    "totalTimeSec",
    "timeToFirstTokenSec",
    "stopReason",
];

fn conversation_file(path: &Path) -> bool {
    if path.components().any(|part| {
        matches!(
            part.as_os_str().to_str(),
            Some("server-logs" | "api-prediction-history" | ".internal" | "credentials")
        )
    }) {
        return false;
    }
    path.file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.ends_with(SUFFIX))
}

fn millis(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .filter(|n| (946_684_800_000..4_102_444_800_000).contains(n))
}

/// A native step identity embeds its creation time, not a request-start timestamp.
fn step_time(id: &str) -> Option<i64> {
    let (time, nonce) = id.split_once('-')?;
    if time.len() != 13
        || !time.bytes().all(|b| b.is_ascii_digit())
        || nonce.is_empty()
        || id.len() > 256
    {
        return None;
    }
    millis(&json!(time.parse::<i64>().ok()?))
}

fn validate(value: &Value) -> Result<()> {
    anyhow::ensure!(
        millis(&value["createdAt"]).is_some(),
        "Missing LM Studio conversation creation time"
    );
    let messages = value["messages"]
        .as_array()
        .context("Missing LM Studio messages")?;
    for message in messages {
        let versions = message["versions"]
            .as_array()
            .context("Missing message versions")?;
        let selected = message["currentlySelected"]
            .as_u64()
            .context("Missing selected version")?;
        anyhow::ensure!(
            !versions.is_empty() && selected < versions.len() as u64,
            "Invalid selected version"
        );
        for version in versions {
            anyhow::ensure!(
                matches!(
                    version["role"].as_str(),
                    Some("user" | "assistant" | "system")
                ),
                "Unsupported message role"
            );
            match version["type"].as_str() {
                Some("singleStep") => {
                    anyhow::ensure!(version["content"].is_array(), "Missing message content")
                }
                Some("multiStep") => {
                    anyhow::ensure!(version["steps"].is_array(), "Missing message steps")
                }
                _ => anyhow::bail!("Unsupported LM Studio message format"),
            }
        }
    }
    Ok(())
}

fn read(path: &Path) -> Result<(Value, String)> {
    anyhow::ensure!(
        conversation_file(path),
        "Only LM Studio .conversation.json files are supported"
    );
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() <= 64 * 1024 * 1024,
        "LM Studio conversation exceeds 64 MiB"
    );
    let value: Value = serde_json::from_slice(&bytes)
        .context("Incomplete or invalid LM Studio conversation; retry after writing finishes")?;
    validate(&value)?;
    Ok((value, files::hash(&bytes)))
}

fn project(value: Value) -> Value {
    // Persistent usage caches must not retain prompts, responses, file references,
    // model configs or even arbitrary fields added to generation stats.
    let messages = value["messages"].as_array().unwrap().iter().map(|message| {
        let versions = message["versions"].as_array().unwrap().iter().map(|version| {
            let steps = version["steps"].as_array().into_iter().flatten().map(|step| {
                let mut projected = json!({"type":step["type"],"stepIdentifier":step["stepIdentifier"]});
                if let Some(info) = step.get("genInfo") {
                    let stats: serde_json::Map<String, Value> = STATS.iter().filter_map(|key| info["stats"].get(*key).map(|v| ((*key).into(),v.clone()))).collect();
                    projected["genInfo"] = json!({"identifier":info["identifier"],"stats":stats});
                }
                projected
            }).collect::<Vec<_>>();
            json!({"role":version["role"],"type":version["type"],"steps":steps})
        }).collect::<Vec<_>>();
        json!({"versions":versions})
    }).collect::<Vec<_>>();
    json!({"createdAt":value["createdAt"],"messages":messages})
}

fn generation(step: &Value, session_id: &str) -> Result<ParsedUsage> {
    let id = step["stepIdentifier"]
        .as_str()
        .context("Missing generation identity")?;
    let at = step_time(id).context("Unrecognized generation identity/time")?;
    let info = &step["genInfo"];
    let stats = &info["stats"];
    let count = |key: &str| {
        stats[key]
            .as_u64()
            .filter(|n| *n <= 1_000_000_000_000)
            .with_context(|| format!("Missing or invalid {key}"))
    };
    let input = count("promptTokensCount")?;
    let output = count("predictedTokensCount")?;
    anyhow::ensure!(
        input.checked_add(output) == Some(count("totalTokensCount")?),
        "Inconsistent generation token total"
    );
    let model = info["identifier"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .context("Missing generation model")?;
    let seconds = |key: &str| {
        stats[key]
            .as_f64()
            .filter(|v| v.is_finite() && *v >= 0.0 && *v <= 3600.0)
            .map(|v| (v * 1000.0).round() as i64)
    };
    let timing = seconds("totalTimeSec")
        .filter(|ms| *ms >= 100)
        .map(|duration_ms| {
            let first = seconds("timeToFirstTokenSec").filter(|ms| *ms <= duration_ms);
            UsageTiming {
                duration_ms,
                first_token_ms: first,
                duration_source: "lmstudio_prediction_stats".into(),
                first_token_source: first.map(|_| "lmstudio_prediction_stats".into()),
            }
        });
    Ok(ParsedUsage {
        id: format!("lmstudio:{id}"),
        native_event_id: id.into(),
        session_id: session_id.into(),
        occurred_at: at / 1000,
        model: model.into(),
        provider: None,
        request_metadata: None,
        reported_cost: None,
        timing,
        // Local prompt/output totals are authoritative. The existing wire format
        // has non-null counters: cache/reasoning placeholders are not observations
        // of zero. UI labels them unreported and pricing is disabled entirely.
        tokens: Tokens {
            input,
            output,
            ..Default::default()
        },
    })
}

fn session_id(value: &Value) -> Option<String> {
    Some(format!("conversation:{}", millis(&value["createdAt"])?))
}

fn content_blocks(content: &Value, thinking: bool) -> Vec<Value> {
    content.as_array().into_iter().flatten().filter_map(|block| {
        match block["type"].as_str() {
            Some("text") => block["text"].as_str().map(|text| json!({"type":if thinking {"thinking"} else {"text"},"text":text})),
            Some("file") => Some(json!({"type":"text","text":format!("[Attachment: {} · {} bytes]",block["fileType"].as_str().unwrap_or("file"),block["sizeBytes"].as_u64().unwrap_or(0))})),
            _ => None,
        }
    }).collect()
}

impl HarnessAdapter for LmStudio {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: "lmstudio",
            name: "LM Studio",
            parser_version: "lmstudio-conversations-v1",
            default_roots: vec![dirs::home_dir()
                .unwrap_or_default()
                .join(".lmstudio/conversations")
                .to_string_lossy()
                .into()],
            usage: true,
            transcript: true,
            delete: false,
        }
    }
    fn allows_price_estimates(&self, _: bool) -> bool {
        false
    }
    fn syncs_timing(&self) -> bool {
        false
    }
    fn discover(&self, root: &Path) -> Result<Vec<PathBuf>> {
        anyhow::ensure!(
            root.exists(),
            "LM Studio conversation directory does not exist"
        );
        if root.is_file() {
            anyhow::ensure!(
                conversation_file(root),
                "Only LM Studio desktop conversation files are supported"
            );
            return Ok(vec![root.into()]);
        }
        let root = if root.join("conversations").is_dir() {
            root.join("conversations")
        } else {
            root.into()
        };
        let mut paths = Vec::new();
        for entry in walkdir::WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| {
                (entry.depth() == 0 || !entry.file_name().to_string_lossy().starts_with('.'))
                    && !matches!(
                        entry.file_name().to_str(),
                        Some("server-logs" | "api-prediction-history" | "credentials")
                    )
            })
        {
            let entry = entry?;
            if entry.file_type().is_file() && conversation_file(entry.path()) {
                paths.push(entry.into_path());
            }
        }
        paths.sort();
        Ok(paths)
    }
    fn read_source(&self, path: &Path) -> Result<(Vec<Value>, String, Vec<String>, bool)> {
        let (value, hash) = read(path)?;
        Ok((vec![value], hash, vec![], true))
    }
    fn read_usage_source(&self, path: &Path) -> Result<(Vec<Value>, String, Vec<String>, bool)> {
        let (value, hash) = read(path)?;
        Ok((vec![project(value)], hash, vec![], true))
    }
    fn usage_reader_version(&self) -> &'static str {
        "lmstudio-usage-v1"
    }
    fn parse(&self, _: &Path, values: &[Value], _: &TimelineIndex) -> Result<ParseResult> {
        let value = values.first().context("Missing conversation")?;
        let session = session_id(value).context("Missing conversation identity")?;
        let mut result = ParseResult {
            complete: true,
            ..Default::default()
        };
        let mut records = BTreeMap::<String, ParsedUsage>::new();
        for message in value["messages"].as_array().context("Missing messages")? {
            for version in message["versions"].as_array().context("Missing versions")? {
                if version["role"] != "assistant" {
                    continue;
                }
                let steps = version["steps"].as_array();
                let generated: Vec<_> = steps
                    .into_iter()
                    .flatten()
                    .filter(|step| step.get("genInfo").is_some())
                    .collect();
                if generated.is_empty() {
                    result.complete = false;
                    result.diagnostics.push(
                        "LM Studio: assistant version has no completed generation statistics"
                            .into(),
                    );
                }
                for step in generated {
                    match generation(step, &session) {
                        Ok(record) => {
                            if let Some(old) = records.get(&record.id) {
                                anyhow::ensure!(
                                    old.tokens == record.tokens
                                        && old.model == record.model
                                        && old.timing == record.timing,
                                    "Conflicting LM Studio generation identity"
                                );
                            } else {
                                records.insert(record.id.clone(), record);
                            }
                        }
                        Err(error) => {
                            result.complete = false;
                            result
                                .diagnostics
                                .push(format!("LM Studio: deferred generation: {error}"));
                        }
                    }
                }
            }
        }
        result.usage = records.into_values().collect();
        Ok(result)
    }
    fn session(&self, path: &Path, values: &[Value]) -> Option<Session> {
        let value = values.first()?;
        let id = session_id(value)?;
        let created = millis(&value["createdAt"])?;
        let latest = ["createdAt", "userLastMessagedAt", "assistantLastMessagedAt"]
            .iter()
            .filter_map(|key| millis(&value[*key]))
            .max();
        Some(Session {
            provider_id: "lmstudio".into(),
            session_id: id,
            title: value["name"].as_str().map(str::to_owned),
            summary: None,
            project_dir: None,
            created_at: Some(created),
            last_active_at: latest,
            source_path: Some(path.to_string_lossy().into()),
            resume_command: None,
            can_delete: false,
        })
    }
    fn messages(&self, values: &[Value]) -> Vec<Value> {
        let Some(value) = values.first() else {
            return vec![];
        };
        let mut out = Vec::new();
        if let Some(prompt) = value["systemPrompt"].as_str().filter(|s| !s.is_empty()) {
            out.push(
                json!({"role":"system","id":"system","blocks":[{"type":"text","text":prompt}]}),
            );
        }
        for (mi, message) in value["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let versions = message["versions"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default();
            for (vi, version) in versions.iter().enumerate() {
                let mut blocks = content_blocks(&version["content"], false);
                let mut at = None;
                let mut meta = json!({});
                for step in version["steps"].as_array().into_iter().flatten() {
                    blocks.extend(content_blocks(
                        &step["content"],
                        step["style"]["type"] == "thinking",
                    ));
                    if let Some(info) = step.get("genInfo") {
                        at = step["stepIdentifier"].as_str().and_then(step_time).or(at);
                        meta = json!({"model":info["identifier"],"inputTokens":info["stats"]["promptTokensCount"],"outputTokens":info["stats"]["predictedTokensCount"],"stopReason":info["stats"]["stopReason"]});
                    }
                }
                let mut message_out = json!({"id":format!("message:{mi}:{vi}"),"role":version["role"],"blocks":blocks,"meta":meta});
                if let Some(at) = at {
                    message_out["ts"] = json!(at);
                }
                if versions.len() > 1 {
                    message_out["versionGroup"] = json!(format!("message:{mi}"));
                    message_out["versionIndex"] = json!(vi);
                    message_out["versionSelected"] =
                        json!(message["currentlySelected"].as_u64() == Some(vi as u64));
                }
                out.push(message_out);
            }
        }
        out
    }
}
