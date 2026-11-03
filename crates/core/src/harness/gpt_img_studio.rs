//! Image Studio's flat, source-estimated model usage. SQLite is authoritative;
//! exported model JSONL uses exactly the same identities and parsing contract.
use super::{files, Descriptor, HarnessAdapter, ParseResult, TimelineIndex};
use crate::{
    model::{ParsedUsage, Session},
    model_usage::{ModelProvider, ModelTokens, ModelUsage, SourcePriceSnapshot},
};
use anyhow::{ensure, Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    time::Duration,
};

pub struct GptImgStudio;

fn default_database() -> PathBuf {
    let base = dirs::data_dir()
        .unwrap_or_default()
        .join("dev.inariaimu.gpt-img-studio");
    let active = std::fs::read(base.join("storage-location.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|v| v["active"].as_str().map(PathBuf::from))
        .filter(|p| p.is_absolute())
        .unwrap_or(base);
    active.join("studio.sqlite3")
}

fn parse_record(v: &Value) -> Result<ParsedUsage> {
    ensure!(
        v["schema"] == "gpt-img-studio.model-usage" && v["schemaVersion"] == 1,
        "Unsupported Image Studio model usage schema"
    );
    let text = |key: &str| -> Result<String> {
        let s = v[key].as_str().context(format!("Missing {key}"))?;
        ensure!(
            !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control),
            "Invalid {key}"
        );
        Ok(s.into())
    };
    let device = text("deviceId")?;
    let native = text("id")?;
    let model = text("model")?;
    let request = text("requestId")?;
    ensure!(
        native.starts_with(&format!("{request}:component:")),
        "Model usage ID differs from request"
    );
    let at = v["occurredAt"]
        .as_i64()
        .filter(|v| *v > 0)
        .context("Invalid usage time")?;
    let source_tokens = &v["tokens"];
    let mut tokens = json!({});
    for key in [
        "input",
        "totalInput",
        "output",
        "cacheRead",
        "cacheWrite",
        "reasoning",
    ] {
        tokens[key] = source_tokens[key].clone();
    }
    let tokens: ModelTokens = serde_json::from_value(tokens)?;
    if let Some(total) = source_tokens["total"].as_u64() {
        ensure!(
            tokens
                .total_input
                .zip(tokens.output)
                .and_then(|(a, b)| a.checked_add(b))
                == Some(total),
            "Model total differs from input/output"
        );
    }
    let price = if v["pricingSnapshot"].is_null() {
        None
    } else {
        let mut p = json!({});
        for key in [
            "source",
            "currency",
            "unit",
            "rates",
            "asOf",
            "serviceTier",
            "contextTier",
        ] {
            if !v["pricingSnapshot"][key].is_null() {
                p[key] = v["pricingSnapshot"][key].clone();
            }
        }
        if let Some(rates) = p["rates"].as_object_mut() {
            rates.retain(|_, v| v.as_str().is_some_and(|s| !s.is_empty()));
        }
        Some(serde_json::from_value::<SourcePriceSnapshot>(p)?)
    };
    ensure!(v["currency"] == "USD", "Unsupported model usage currency");
    let usage = ModelUsage {
        provider: if v["provider"].is_object() {
            Some(ModelProvider {
                id: v["provider"]["id"]
                    .as_str()
                    .context("Missing provider ID")?
                    .into(),
                name: v["provider"]["name"]
                    .as_str()
                    .context("Missing provider name")?
                    .into(),
                kind: v["provider"]["kind"]
                    .as_str()
                    .context("Missing provider kind")?
                    .into(),
                api_service: v["provider"]["apiService"]
                    .as_str()
                    .unwrap_or("unknown")
                    .into(),
            })
        } else {
            None
        },
        source_device_id: device.clone(),
        request_id: request,
        operation: text("operation")?,
        kind: text("kind")?,
        usage_available: v["usageAvailable"]
            .as_bool()
            .context("Missing availability")?,
        cache_breakdown_known: v["cacheBreakdownKnown"]
            .as_bool()
            .context("Missing cache availability")?,
        cache_write_applicable: v["cacheWriteApplicable"]
            .as_bool()
            .context("Missing cache write applicability")?,
        tokens,
        text_input_tokens: serde_json::from_value(v["textInputTokens"].clone())?,
        image_input_tokens: serde_json::from_value(v["imageInputTokens"].clone())?,
        estimated_cost_usd: serde_json::from_value(v["estimatedCostUsd"].clone())?,
        estimate_status: text("estimateStatus")?,
        pricing_snapshot: price,
        status_code: serde_json::from_value(v["statusCode"].clone())?,
        request_duration_ms: serde_json::from_value(v["requestDurationMs"].clone())?,
    };
    usage.validate()?;
    // Keep API routes separate from Codex's `openai` subscription route.
    // Include known service identity so editing a connection cannot merge its
    // old unknown endpoint with newly captured OpenAI Platform traffic.
    let provider = usage.provider.as_ref().map(ModelProvider::route);
    ensure!(
        provider.as_deref().is_none_or(crate::providers::valid),
        "Invalid Image Studio provider"
    );
    Ok(ParsedUsage {
        id: format!("gpt-img-studio:{device}:{native}"),
        native_event_id: native,
        session_id: format!("{device}:{}", text("sessionId")?),
        occurred_at: at,
        model,
        tokens: usage.projection(),
        model_usage: Some(usage),
        provider,
        request_metadata: None,
        reported_cost: None,
        timing: None,
    })
}

impl HarnessAdapter for GptImgStudio {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: "gpt-img-studio",
            name: "GPT Image Studio",
            parser_version: "gpt-img-studio-model-v2",
            default_roots: vec![default_database().to_string_lossy().into()],
            usage: true,
            transcript: false,
            delete: false,
        }
    }
    fn allows_price_estimates(&self, _: bool) -> bool {
        false
    }
    fn discover(&self, root: &Path) -> Result<Vec<PathBuf>> {
        if root.is_file() {
            ensure!(
                root.file_name().is_some_and(|n| n == "studio.sqlite3")
                    || root.extension().is_some_and(|e| e == "jsonl"),
                "Expected studio.sqlite3 or model usage JSONL"
            );
            return Ok(vec![root.into()]);
        }
        ensure!(root.is_dir(), "Image Studio data path does not exist");
        let db = root.join("studio.sqlite3");
        if db.is_file() {
            return Ok(vec![db]);
        }
        for log in [root.join("models.jsonl"), root.join("usage/models.jsonl")] {
            if log.is_file() {
                return Ok(vec![log]);
            }
        }
        anyhow::bail!("No Image Studio model usage found; restart the updated Image Studio first")
    }
    fn read_source(&self, path: &Path) -> Result<(Vec<Value>, String, Vec<String>, bool)> {
        if path.extension().is_some_and(|e| e == "jsonl") {
            return files::read(path);
        }
        let mut db = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        db.busy_timeout(Duration::from_secs(5))?;
        let tx = db.transaction()?;
        let mut stmt = tx
            .prepare("SELECT measurement FROM model_usage ORDER BY occurred_at,id")
            .context(
                "Image Studio model usage table is unavailable; restart the updated application",
            )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut events = Vec::new();
        for row in rows {
            events.push(serde_json::from_str::<Value>(&row?)?);
        }
        let fingerprint = files::hash(&serde_json::to_vec(&events)?);
        Ok((events, fingerprint, Vec::new(), true))
    }
    fn parse(&self, _: &Path, lines: &[Value], _: &TimelineIndex) -> Result<ParseResult> {
        let mut result = ParseResult {
            complete: true,
            ..Default::default()
        };
        let mut records: BTreeMap<String, ParsedUsage> = BTreeMap::new();
        let mut conflicts = HashSet::new();
        for (index, v) in lines.iter().enumerate() {
            match parse_record(v) {
                Ok(record) => {
                    if conflicts.contains(&record.id) {
                        continue;
                    }
                    if let Some(old) = records.get(&record.id) {
                        if old.model_usage != record.model_usage
                            || old.model != record.model
                            || old.occurred_at != record.occurred_at
                            || old.provider != record.provider
                            || old.session_id != record.session_id
                        {
                            result.complete = false;
                            result.diagnostics.push(format!(
                                "Conflicting Image Studio measurement on row {}",
                                index + 1
                            ));
                            result.retained_ids.push(record.id.clone());
                            records.remove(&record.id);
                            conflicts.insert(record.id);
                        }
                    } else {
                        records.insert(record.id.clone(), record);
                    }
                }
                Err(error) => {
                    result.complete = false;
                    result
                        .diagnostics
                        .push(format!("Image Studio row {}: {error}", index + 1));
                }
            }
        }
        result.usage = records.into_values().collect();
        Ok(result)
    }
    fn session(&self, _: &Path, _: &[Value]) -> Option<Session> {
        None
    }
    fn messages(&self, _: &[Value]) -> Vec<Value> {
        Vec::new()
    }
}
