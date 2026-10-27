//! Codex rollout identities, continuation baselines and replay accounting.
//! Adapted from CC Switch v4.0.18 (MIT, Copyright (c) 2025 Jason Young).
//! Persistence, original provider/model labels and strict malformed-data handling
//! remain Harness Scope responsibilities. See third_party/UPSTREAM.md.
use super::{ParseResult, TimelineIndex};
use crate::model::{ParsedUsage, Tokens};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
};

pub const VERSION: &str = "codex-v7-request-settings";
type Counters = [u64; 5]; // total input, output, cache read, cache write, reasoning

#[derive(Clone, Debug)]
pub(crate) struct Identity {
    pub session: String,
    pub rollout: String,
    pub parent: Option<String>,
    pub continuation: bool,
    pub history_base: Option<String>,
}
fn string(v: &Value) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|s| {
            uuid::Uuid::parse_str(s)
                .map(|u| u.to_string())
                .unwrap_or_else(|_| s.into())
        })
}
pub(crate) fn logical_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .trim_end_matches(".zst")
        .to_owned()
}
pub(crate) fn identity(path: &Path, events: &[Value]) -> Result<Identity> {
    let meta = events
        .iter()
        .find(|v| v["type"] == "session_meta")
        .context("Missing Codex session_meta")?;
    let p = &meta["payload"];
    let session = string(&p["id"])
        .or_else(|| string(&p["thread_id"]))
        .or_else(|| string(&p["threadId"]))
        .context("Missing Codex thread ID")?;
    let name = logical_name(path);
    let stem = name.strip_suffix(".jsonl").unwrap_or(&name);
    let uuid_at = |offset: usize| {
        stem.get(offset..)
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
            .map(|u| u.to_string())
    };
    let rollout = stem.len().checked_sub(36).and_then(uuid_at);
    let leading = stem.len().checked_sub(73).and_then(|start| {
        let pair = stem.get(start..)?;
        (pair.as_bytes().get(36) == Some(&b'_'))
            .then(|| pair.get(..36).and_then(|s| uuid::Uuid::parse_str(s).ok()))
            .flatten()
            .map(|u| u.to_string())
    });
    if let Some(id) = &rollout {
        anyhow::ensure!(
            id == &session || leading.as_deref() == Some(&session),
            "Codex filename/meta thread identity mismatch"
        );
    }
    let parents = [
        string(&p["forked_from_id"]),
        string(&p["parent_thread_id"]),
        p.pointer("/source/subagent/thread_spawn/parent_thread_id")
            .and_then(string),
    ];
    let parent = parents.iter().flatten().next().cloned();
    anyhow::ensure!(
        parents.iter().flatten().all(|v| Some(v) == parent.as_ref()),
        "Conflicting Codex parent identities"
    );
    let history_base = p.pointer("/history_base/thread_id").and_then(string);
    let continuation = if leading.as_deref() == Some(&session) {
        p["history_mode"] == "paginated" || history_base.is_some() || parent.is_some()
    } else {
        history_base.as_deref() == Some(&session)
    };
    anyhow::ensure!(
        continuation || parent.as_deref() != Some(&session),
        "Codex parent is the current thread"
    );
    Ok(Identity {
        rollout: rollout.unwrap_or_else(|| session.clone()),
        session,
        parent,
        continuation,
        history_base,
    })
}
pub(crate) fn dependencies(id: &Identity) -> Vec<String> {
    let mut ids = vec![id.session.clone()];
    if !id.continuation {
        ids.extend(id.parent.iter().cloned());
    }
    if id.continuation {
        ids.extend(id.history_base.iter().cloned());
    }
    ids.sort();
    ids.dedup();
    ids
}
fn timestamp(v: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(v.as_str()?)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}
fn is_usage(v: &Value) -> bool {
    v["type"] == "event_msg"
        && v["payload"]["type"] == "token_count"
        && v["payload"]["info"].is_object()
}

/// Retain no transcript content, but retain the latest timestamp from all events:
/// a parent's final task marker can establish that it reached a fork cutoff.
pub(crate) fn context(events: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    for event in events {
        let p = &event["payload"];
        let payload = if is_usage(event) {
            json!({"type": "token_count", "info": p["info"]})
        } else if event["type"] == "session_meta" {
            json!({
                "id": p["id"], "thread_id": p["thread_id"], "threadId": p["threadId"],
                "forked_from_id": p["forked_from_id"], "parent_thread_id": p["parent_thread_id"],
                "source": {"subagent": {"thread_spawn": {
                    "parent_thread_id": p.pointer("/source/subagent/thread_spawn/parent_thread_id")
                }}},
                "history_mode": p["history_mode"], "history_base": p["history_base"]
            })
        } else {
            continue;
        };
        out.push(
            json!({"type": event["type"], "timestamp": event["timestamp"], "payload": payload}),
        );
    }
    if let Some(at) = events
        .iter()
        .filter_map(|v| timestamp(&v["timestamp"]))
        .max()
    {
        out.push(json!({"type":"scope_context_end","timestamp":at.to_rfc3339()}));
    }
    out
}
pub(crate) fn add_context(
    index: &mut TimelineIndex,
    path: &Path,
    events: &[Value],
    available: bool,
) -> Result<()> {
    let id = identity(path, events)?;
    let wrapper =
        json!({"_codexPath":path.to_string_lossy(),"events":events,"available":available});
    for key in [id.session.clone(), id.rollout]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
    {
        index
            .entry(format!("codex:{key}"))
            .or_default()
            .push(wrapper.clone());
    }
    Ok(())
}
fn segments<'a>(index: &'a TimelineIndex, id: &str) -> Result<Vec<(String, &'a [Value])>> {
    let rows = index
        .get(&format!("codex:{id}"))
        .with_context(|| format!("Missing Codex context {id}"))?;
    // Compatibility with pre-v6 cached timelines and adapter callers.
    if rows.first().is_none_or(|v| v.get("_codexPath").is_none()) {
        return Ok(vec![(format!("{id}.jsonl"), rows)]);
    }
    let mut out = Vec::new();
    for row in rows {
        anyhow::ensure!(
            row["available"] == true,
            "Codex context {id} is being written or is unreadable"
        );
        out.push((
            row["_codexPath"]
                .as_str()
                .context("Invalid Codex context path")?
                .to_owned(),
            row["events"]
                .as_array()
                .context("Invalid Codex context events")?
                .as_slice(),
        ));
    }
    out.sort_by_key(|(p, _)| logical_name(Path::new(p)));
    Ok(out)
}
fn counters(value: &Value) -> Result<Option<Counters>> {
    let Some(fields) = value.as_object() else {
        return Ok(None);
    };
    if ![
        "input_tokens",
        "cached_input_tokens",
        "cache_read_input_tokens",
        "output_tokens",
        "reasoning_output_tokens",
        "total_tokens",
    ]
    .iter()
    .any(|k| fields.contains_key(*k))
    {
        return Ok(None);
    }
    for (key, v) in fields {
        if key.ends_with("tokens") && !v.is_null() {
            anyhow::ensure!(
                v.as_u64().is_some_and(|n| n <= 1_000_000_000_000),
                "Invalid Codex token counter {key}"
            );
        }
    }
    let n = |key: &str| value[key].as_u64().unwrap_or(0);
    Ok(Some([
        n("input_tokens"),
        n("output_tokens"),
        value["cached_input_tokens"]
            .as_u64()
            .unwrap_or_else(|| n("cache_read_input_tokens")),
        value["cache_write_input_tokens"]
            .as_u64()
            .unwrap_or_else(|| n("cache_creation_input_tokens")),
        n("reasoning_output_tokens"),
    ]))
}
fn signature(info: &Value) -> String {
    let sig = |v: &Value| {
        v.as_object().map(|_| {
            json!([
                v["input_tokens"].as_u64(),
                v["cached_input_tokens"]
                    .as_u64()
                    .or_else(|| v["cache_read_input_tokens"].as_u64()),
                v["output_tokens"].as_u64(),
                v["reasoning_output_tokens"].as_u64(),
                v["total_tokens"].as_u64()
            ])
        })
    };
    json!([
        sig(&info["total_token_usage"]),
        sig(&info["last_token_usage"])
    ])
    .to_string()
}
fn high_water(events: &[Value]) -> Result<Option<Counters>> {
    let mut high = None;
    for v in events.iter().filter(|v| is_usage(v)) {
        if let Some(c) = counters(&v["payload"]["info"]["total_token_usage"])? {
            update_high(&mut high, c);
        }
    }
    Ok(high)
}
fn update_high(high: &mut Option<Counters>, c: Counters) {
    *high = Some(high.map_or(c, |h| std::array::from_fn(|i| h[i].max(c[i]))));
}
fn continuation_baseline(
    path: &Path,
    id: &Identity,
    index: &TimelineIndex,
) -> Result<Option<Counters>> {
    for key in [&id.session, id.history_base.as_ref().unwrap_or(&id.session)] {
        if let Ok(rows) = segments(index, key) {
            for (_, events) in rows
                .iter()
                .rev()
                .filter(|(p, _)| logical_name(Path::new(p)) < logical_name(path))
            {
                if let Some(high) = high_water(events)? {
                    return Ok(Some(high));
                }
            }
        } else if index.contains_key(&format!("codex:{key}")) {
            // An unavailable predecessor cannot be treated as an empty history.
            segments(index, key)?;
        }
    }
    anyhow::ensure!(
        id.history_base.is_none(),
        "Missing Codex continuation baseline for {}",
        id.session
    );
    Ok(None)
}
fn parent_signatures(
    index: &TimelineIndex,
    id: &str,
    cutoff: DateTime<Utc>,
) -> Result<Vec<String>> {
    let mut merged = Vec::new();
    let mut seen: BTreeMap<String, (Vec<String>, Option<DateTime<Utc>>)> = BTreeMap::new();
    for (path, events) in segments(index, id)? {
        let rollout = identity(Path::new(&path), events)
            .map(|i| i.rollout)
            .unwrap_or_else(|_| id.into());
        let end = events
            .iter()
            .filter_map(|v| timestamp(&v["timestamp"]))
            .max();
        let mut signatures = Vec::new();
        for v in events.iter().filter(|v| is_usage(v)) {
            let at =
                timestamp(&v["timestamp"]).context("Parent Codex usage has no valid timestamp")?;
            if at <= cutoff {
                signatures.push(signature(&v["payload"]["info"]));
            }
        }
        if let Some((old, max)) = seen.get_mut(&rollout) {
            anyhow::ensure!(
                *old == signatures,
                "Conflicting copies of Codex parent rollout {rollout}"
            );
            *max = max.zip(end).map(|(a, b)| a.min(b));
        } else {
            merged.extend(signatures.clone());
            seen.insert(rollout, (signatures, end));
        }
    }
    anyhow::ensure!(
        seen.values()
            .filter_map(|(_, end)| *end)
            .max()
            .is_some_and(|end| end >= cutoff),
        "Codex parent has not reached the fork timestamp"
    );
    Ok(merged)
}

/// An explicit history_base names the exact parent snapshot, not the child's
/// creation time. Validate it against the physical parent before trusting it.
fn bounded_parent(
    index: &TimelineIndex,
    parent: &str,
    base: &Value,
    cutoff: DateTime<Utc>,
) -> Result<(Vec<String>, Option<Counters>)> {
    anyhow::ensure!(
        string(&base["thread_id"]).as_deref() == Some(parent),
        "Codex history boundary parent mismatch"
    );
    let bytes = base["end_byte_offset"]
        .as_u64()
        .context("Invalid Codex parent byte boundary")?;
    let ordinal = base["end_ordinal_exclusive"]
        .as_u64()
        .context("Invalid Codex parent ordinal boundary")?;
    let mut rollout = None;
    let mut digest = None;
    let mut result = None;
    for (path, context) in segments(index, parent)? {
        let id = identity(Path::new(&path), context)?;
        anyhow::ensure!(
            id.session == parent,
            "Codex history boundary does not identify the parent thread"
        );
        if let Some(previous) = &rollout {
            anyhow::ensure!(
                previous == &id.rollout,
                "Codex parent boundary is ambiguous across rollout segments"
            );
        } else {
            rollout = Some(id.rollout.clone());
        }
        let (events, hash) = super::usage_read::codex_prefix(Path::new(&path), bytes, ordinal)?;
        let verified = identity(Path::new(&path), &events)?;
        anyhow::ensure!(
            verified.session == parent && verified.rollout == id.rollout,
            "Codex parent history identity changed"
        );
        anyhow::ensure!(
            events
                .iter()
                .filter_map(|v| timestamp(&v["timestamp"]))
                .all(|at| at <= cutoff),
            "Codex parent boundary contains events after the fork"
        );
        if let Some(previous) = &digest {
            anyhow::ensure!(previous == &hash, "Conflicting Codex parent history copies");
        } else {
            let mut signatures = Vec::new();
            for event in events.iter().filter(|v| is_usage(v)) {
                timestamp(&event["timestamp"])
                    .context("Parent Codex usage has no valid timestamp")?;
                signatures.push(signature(&event["payload"]["info"]));
            }
            result = Some((signatures, high_water(&events)?));
            digest = Some(hash);
        }
    }
    result.context("Missing Codex parent history boundary")
}
fn make_usage(
    id: &Identity,
    suffix: String,
    at: &Value,
    model: &str,
    provider: Option<String>,
    c: Counters,
) -> Result<ParsedUsage> {
    let read = c[2].min(c[0]);
    let write = c[3].min(c[0] - read);
    Ok(ParsedUsage {
        id: format!("codex:v6:{}:{suffix}", id.rollout),
        session_id: id.session.clone(),
        native_event_id: suffix,
        occurred_at: timestamp(at)
            .context("Codex usage has no valid timestamp")?
            .timestamp(),
        model: model.into(),
        provider,
        tokens: Tokens {
            input: c[0] - read - write,
            output: c[1],
            cache_read: read,
            cache_write: write,
            cache_write1h: 0,
            reasoning: c[4].min(c[1]),
        },
        reported_cost: None,
        timing: None,
        request_metadata: None,
    })
}
fn nonzero(c: Counters) -> bool {
    // Cached input is clamped to total input before billability is decided.
    c[0] > 0 || c[1] > 0
}

pub(crate) fn parse(path: &Path, lines: &[Value], index: &TimelineIndex) -> Result<ParseResult> {
    match parse_complete(path, lines, index) {
        Ok(out) => Ok(out),
        Err(e) => Ok(ParseResult {
            complete: false,
            diagnostics: vec![format!("{}: {e:#}", path.display())],
            ..Default::default()
        }),
    }
}
fn parse_complete(path: &Path, lines: &[Value], index: &TimelineIndex) -> Result<ParseResult> {
    let id = identity(path, lines)?;
    let meta = lines.iter().find(|v| v["type"] == "session_meta").unwrap();
    let base = &meta["payload"]["history_base"];
    let has_boundary = !id.continuation
        && (!base["end_byte_offset"].is_null() || !base["end_ordinal_exclusive"].is_null());
    if has_boundary {
        anyhow::ensure!(
            id.parent.is_some(),
            "Codex history boundary has no parent thread"
        );
    }
    let mut boundary_high = None;
    let parent = if !id.continuation {
        id.parent
            .as_ref()
            .map(|p| {
                let cutoff =
                    timestamp(&meta["timestamp"]).context("Missing Codex fork timestamp")?;
                let base = &meta["payload"]["history_base"];
                if !base["end_byte_offset"].is_null() || !base["end_ordinal_exclusive"].is_null() {
                    let (signatures, high) = bounded_parent(index, p, base, cutoff)?;
                    boundary_high = high;
                    Ok(signatures)
                } else {
                    parent_signatures(index, p, cutoff)
                }
            })
            .transpose()?
    } else {
        None
    };
    let mut parent_offset = 0;
    let mut replay = parent.is_some();
    let mut high = None;
    let mut last_by_source = HashMap::new();
    let mut previous = None;
    let mut event_index = 0;
    let mut model = "unknown".to_owned();
    let mut provider = crate::providers::parse(meta["payload"]["model_provider"].as_str());
    let mut timer = super::codex_timing::RequestTimer::default();
    let mut settings = super::codex_settings::Settings::default();
    let mut pending: Option<(Counters, ParsedUsage)> = None;
    let mut out = ParseResult {
        complete: true,
        ..Default::default()
    };
    for (line, v) in lines.iter().enumerate() {
        let p = &v["payload"];
        timer.observe(v);
        settings.observe(v);
        if v["type"] == "session_meta" || v["type"] == "turn_context" {
            if let Some(value) = crate::providers::parse(p["model_provider"].as_str()) {
                provider = Some(value);
            }
            if let Some(value) = p["model"]
                .as_str()
                .or_else(|| p.pointer("/info/model").and_then(Value::as_str))
                .filter(|s| !s.trim().is_empty())
            {
                model = value.into();
            }
        }
        if v["type"] == "event_msg" && p["type"] == "thread_settings_applied" {
            if let Some(value) = p
                .pointer("/thread_settings/model")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
            {
                model = value.into();
            }
        }
        if v["type"] == "token_usage_record" {
            pending = None;
            if let Some(c) = counters(&p["usage"])?.filter(|c| nonzero(*c)) {
                let suffix = p["response_id"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        format!(
                            "line{}",
                            v["_scopeLine"].as_u64().unwrap_or(line as u64 + 1)
                        )
                    });
                let mut usage = make_usage(
                    &id,
                    format!("usage:{suffix}"),
                    &v["timestamp"],
                    &model,
                    provider.clone(),
                    c,
                )?;
                usage.request_metadata = settings.metadata(v, &model);
                pending = Some((c, usage));
            }
        }
        if !is_usage(v) {
            continue;
        }
        let info = &p["info"];
        if let Some(value) = info["model"]
            .as_str()
            .or_else(|| info["model_name"].as_str())
            .or_else(|| p["model"].as_str())
            .filter(|s| !s.trim().is_empty())
        {
            model = value.into();
        }
        let total = counters(&info["total_token_usage"])?;
        let last = counters(&info["last_token_usage"])?;
        let supplemental = if let Some((c, usage)) = pending.take() {
            let covered = last.is_some_and(|l| l[..4] == c[..4]);
            if !covered && last.is_some_and(|l| l[..4].iter().all(|n| *n == 0)) {
                Some(usage)
            } else {
                None
            }
        } else {
            None
        };
        if total.is_none() && last.is_none() {
            continue;
        }
        let sig = signature(info);
        let lane = p
            .pointer("/rate_limits/limit_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let duplicate = total.is_some()
            && (last_by_source.get(&lane) == Some(&sig) || previous.as_ref() == Some(&sig));
        if total.is_some() {
            last_by_source.insert(lane, sig.clone());
        }
        previous = Some(sig.clone());
        let inherited = if replay {
            let parent = parent.as_ref().unwrap();
            if let Some(n) = parent[parent_offset..].iter().position(|s| s == &sig) {
                parent_offset += n + 1;
                true
            } else {
                replay = false;
                false
            }
        } else {
            false
        };
        if !inherited {
            if let Some(usage) = supplemental {
                out.usage.push(usage);
            }
        }
        let delta = if duplicate {
            [0; 5]
        } else if let Some(c) = last {
            c
        } else if let Some(total) = total {
            if id.continuation && high.is_none() {
                high = continuation_baseline(path, &id, index)?.filter(|h| total[0] >= h[0]);
            }
            if !inherited && has_boundary {
                let baseline = boundary_high.context(
                    "Missing Codex parent cumulative baseline for total-only fork usage",
                )?;
                // Replayed child history may be only a prefix of the captured
                // parent snapshot. Use the full snapshot high-water mark.
                if total[0] >= baseline[0] {
                    update_high(&mut high, baseline);
                }
            }
            high.map_or(total, |h: Counters| {
                std::array::from_fn(|i| total[i].saturating_sub(h[i]))
            })
        } else {
            unreachable!()
        };
        if let Some(c) = total {
            update_high(&mut high, c);
        }
        if !nonzero(delta) {
            continue;
        }
        event_index += 1;
        let timing = timer.finish(v, delta);
        if inherited {
            continue;
        }
        let mut usage = make_usage(
            &id,
            format!("token:{event_index}"),
            &v["timestamp"],
            &model,
            provider.clone(),
            delta,
        )?;
        usage.timing = timing;
        usage.request_metadata = settings.metadata(v, &model);
        out.usage.push(usage);
    }
    Ok(out)
}
