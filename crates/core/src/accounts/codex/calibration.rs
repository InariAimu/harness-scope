//! Calibrate model-relative estimates against changes in the same account/window.
use super::*;
use crate::query_store::Row;
use rust_decimal::prelude::ToPrimitive;
const MAX_POINTS: usize = 4096;
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Observation {
    at: i64,
    identity: String,
    plan: String,
    bucket: String,
    minutes: i64,
    reset: i64,
    used: String,
}
fn key(id: &str) -> String {
    format!("codex_quota_history_v1:{id}")
}
pub(crate) fn clear(store: &Store, id: &str) -> Result<()> {
    store
        .conn
        .execute("DELETE FROM meta WHERE key=?", [key(id)])?;
    Ok(())
}
fn identity(data: &Value) -> Option<&str> {
    data["identity"].as_str().filter(|s| {
        data["identitySource"] == "accountId"
            && s.len() == 64
            && s.bytes().all(|b| b.is_ascii_hexdigit())
    })
}
fn points(snapshot: &Value) -> Vec<Observation> {
    let data = &snapshot["data"];
    let Some(identity) = identity(data) else {
        return vec![];
    };
    let Some(at) = snapshot["fetchedAt"].as_i64() else {
        return vec![];
    };
    data["buckets"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|b| {
            b["windows"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(move |w| {
                    Some(Observation {
                        at,
                        identity: identity.into(),
                        plan: data["planType"].as_str().unwrap_or("").into(),
                        bucket: b["id"].as_str()?.into(),
                        minutes: w["durationMinutes"].as_i64()?,
                        reset: w["resetsAt"].as_i64()?,
                        used: w["usedPercent"].to_string(),
                    })
                })
        })
        .collect()
}
fn same(a: &Observation, b: &Observation) -> bool {
    a.identity == b.identity
        && a.plan == b.plan
        && a.bucket == b.bucket
        && a.minutes == b.minutes
        && quota_history::same_codex_reset(a.reset, b.reset)
}
pub(crate) fn record(
    store: &Store,
    id: &str,
    previous: Option<&Value>,
    current: &Value,
) -> Result<()> {
    let mut history: Vec<Observation> = store.meta(&key(id))?.unwrap_or_default();
    let now = current["fetchedAt"]
        .as_i64()
        .context("Invalid Codex observation time")?;
    let Some(current_identity) = identity(&current["data"]) else {
        return Ok(());
    };
    history.retain(|p| p.identity == current_identity && p.at >= now - 8 * 86400 && p.at <= now);
    for point in previous.into_iter().flat_map(points).chain(points(current)) {
        if point.identity != current_identity || point.at < now - 8 * 86400 || point.at > now {
            continue;
        }
        let Ok(value) = Decimal::from_str(&point.used) else {
            continue;
        };
        let indexes: Vec<_> = history
            .iter()
            .enumerate()
            .filter(|(_, p)| same(p, &point))
            .map(|(i, _)| i)
            .collect();
        if let Some(&last) = indexes.last() {
            if history[last].at > point.at {
                continue;
            }
            let before = Decimal::from_str(&history[last].used).unwrap_or_default();
            if value < before && !quota_history::is_adjustment(before, value) {
                // This cache is derived; the complete observation remains in history.
                continue;
            } else if history[last].at == point.at {
                history[last] = point;
                continue;
            } else if indexes.len() >= 2
                && history[indexes[indexes.len() - 2]].used == point.used
                && history[last].used == point.used
                && history[indexes[indexes.len() - 2]].at / 60 == point.at / 60
                && history[last].at / 60 == point.at / 60
            {
                // Keep minute anchors even across long flat percentages. Collapsing
                // the whole plateau would turn the next rounded jump into a spike.
                history.remove(last);
            }
        }
        history.push(point);
    }
    history.sort_by_key(|p| p.at);
    if history.len() > MAX_POINTS {
        history.drain(..history.len() - MAX_POINTS);
    }
    store.set_meta(&key(id), &history)
}
pub(super) struct Input<'a> {
    pub id: &'a str,
    pub snapshot: Option<&'a Value>,
    pub bucket: &'a str,
    pub catalog: &'a catalog::Catalog,
    pub plan: &'a str,
    pub mode: &'a str,
    pub model: Option<&'a catalog::Model>,
    pub now: i64,
    pub lookback: i64,
    pub rate: Decimal,
    pub request_rate: Decimal,
    pub pace: Decimal,
    pub sample_valid: bool,
}
fn mul(a: Decimal, b: Decimal) -> Result<Decimal> {
    a.checked_mul(b).context("Calibration precision exceeded")
}
fn div(a: Decimal, b: Decimal) -> Result<Decimal> {
    a.checked_div(b).context("Calibration precision exceeded")
}
fn decimal(d: Decimal) -> String {
    d.round_dp(18).normalize().to_string()
}
pub(super) fn calculate(
    store: &Store,
    input: Input<'_>,
    events: &[&Row],
    quote: impl Fn(&Row) -> Option<Decimal>,
) -> Result<Value> {
    let mut history: Vec<Observation> = store.meta(&key(input.id))?.unwrap_or_default();
    // Existing installations retain their old calibration cache as a fallback. Once real
    // observations exist, both local and synchronized samples use one source of truth.
    let raw = crate::accounts::history::between(
        store,
        input.id,
        (input.now - 8 * 86400) * 1000,
        input.now * 1000 + 999,
    )?;
    if !raw.is_empty() {
        history.clear();
        let mut revisions = std::collections::BTreeMap::new();
        for sample in raw {
            let source = sample["deviceId"].as_str().unwrap_or("local").to_owned();
            let revision = sample["accountRevision"].clone();
            if revisions
                .insert(source, revision.clone())
                .is_some_and(|old| old != revision)
            {
                history.clear();
            }
            history.extend(points(&json!({"fetchedAt":sample["observedAtMs"].as_i64().unwrap_or(0)/1000,"data":sample["data"]})));
        }
        history.sort_by_key(|p| p.at);
        history.dedup();
    }
    let snapshot = input.snapshot;
    let data = snapshot.map(|s| &s["data"]);
    let who = data.and_then(identity);
    let current = snapshot.map(points).unwrap_or_default();
    let mut windows = Vec::new();
    for point in current.iter().filter(|p| p.bucket == input.bucket) {
        let id = if point.minutes == 300 {
            "rolling"
        } else if point.minutes == 10080 {
            "weekly"
        } else {
            "other"
        };
        let mut result = json!({"id":id,"durationMinutes":point.minutes,"resetsAt":point.reset,"status":"unavailable","reason":"insufficient_quota_history","observations":0});
        let estimates = (|| -> Result<Value> {
            anyhow::ensure!(
                snapshot
                    .and_then(|s| s["fetchedAt"].as_i64())
                    .is_some_and(|at| at <= input.now && input.now - at <= 600)
                    && point.reset > input.now,
                "stale_quota"
            );
            let mut samples: Vec<_> = history
                .iter()
                .filter(|p| {
                    same(p, point)
                        && p.at
                            >= (input.now - input.lookback).max(point.reset - point.minutes * 60)
                        && p.at <= point.at
                })
                .collect();
            samples.sort_by_key(|p| p.at);
            let mut high_water = Decimal::ZERO;
            let mut segment_start = 0;
            let mut ignored = 0;
            let mut accepted = Vec::new();
            for p in samples {
                let Ok(used) = Decimal::from_str(&p.used) else {
                    ignored += 1;
                    continue;
                };
                if used < Decimal::ZERO || used > Decimal::from(100) {
                    ignored += 1;
                    continue;
                }
                if quota_history::is_adjustment(high_water, used) {
                    // Earlier observations are valid, but belong to an older
                    // measurement interval and must not price this interval.
                    segment_start = accepted.len();
                } else if used < high_water {
                    ignored += 1;
                    continue;
                }
                accepted.push(p);
                high_water = used;
            }
            let samples = &accepted[segment_start..];
            result["ignoredObservations"] = json!(ignored);
            result["observations"] = json!(samples.len());
            anyhow::ensure!(samples.len() >= 2, "insufficient_quota_history");
            let first = samples[0];
            let last = samples[samples.len() - 1];
            let seconds = last.at - first.at;
            result["sampleStart"] = json!(first.at);
            result["sampleEnd"] = json!(last.at);
            result["spanSeconds"] = json!(seconds);
            anyhow::ensure!(seconds >= 60, "insufficient_quota_history");
            let delta = Decimal::from_str(&last.used)? - Decimal::from_str(&first.used)?;
            result["deltaPercent"] = json!(decimal(delta));
            anyhow::ensure!(delta >= Decimal::ONE, "no_quota_growth");
            let observed = mul(div(delta, Decimal::from(seconds))?, Decimal::from(3600))?;
            result["observedPercentPerHour"] = json!(decimal(observed));
            anyhow::ensure!(
                input.catalog.bucket_ids.iter().any(|b| b == input.bucket),
                "unsupported_bucket"
            );
            let mut normalized = Decimal::ZERO;
            let mut usd = Decimal::ZERO;
            let mut requests = 0u64;
            let mut unknown = 0u64;
            let mut unknown_speed = 0u64;
            let mut models = std::collections::BTreeSet::new();
            for row in events
                .iter()
                .filter(|r| r.occurred_at > first.at && r.occurred_at <= last.at)
            {
                requests = requests.saturating_add(row.count);
                let cost = quote(row);
                let m = catalog::find(input.catalog, &row.model);
                let capacity = m
                    .map(|m| catalog::resolve(input.catalog, m, input.plan, id, input.mode))
                    .transpose()?
                    .flatten();
                let speed = row.request_metadata.as_ref().and_then(|s| s.speed());
                let multiplier = match speed {
                    Some("standard") => Some(Decimal::ONE),
                    Some("fast")
                        if m.is_some_and(|m| {
                            input
                                .catalog
                                .subscription_speed
                                .fast_model_ids
                                .contains(&m.id)
                        }) =>
                    {
                        Some(Decimal::from_str(
                            &input.catalog.subscription_speed.fast_multiplier,
                        )?)
                    }
                    _ => None,
                };
                if multiplier.is_none() {
                    unknown_speed = unknown_speed.saturating_add(row.count);
                }
                match cost.zip(capacity) {
                    Some((cost, capacity)) => {
                        // Fit a Standard-equivalent coefficient across mixed tiers.
                        // Applying Fast to a future row then happens exactly once.
                        let n = mul(
                            div(cost, Decimal::from_str(&capacity.api_equivalent_usd)?)?,
                            multiplier.unwrap_or(Decimal::ONE),
                        )?;
                        normalized = normalized
                            .checked_add(n)
                            .context("Calibration precision exceeded")?;
                        usd = usd
                            .checked_add(cost)
                            .context("Calibration precision exceeded")?;
                        models.insert(m.unwrap().id.as_str());
                    }
                    None => unknown = unknown.saturating_add(row.count),
                }
            }
            result["linkedRequests"] = json!(requests);
            result["unpricedRequests"] = json!(unknown);
            result["linkedUsd"] = json!(decimal(usd));
            result["modelCount"] = json!(models.len());
            result["unknownSpeedRequests"] = json!(unknown_speed);
            result["speedBaseline"] = json!("standard");
            anyhow::ensure!(unknown == 0, "unpriced_calibration");
            anyhow::ensure!(unknown_speed == 0, "unknown_speed_baseline");
            anyhow::ensure!(
                requests > 0 && normalized > Decimal::ZERO,
                "no_linked_calibration_usage"
            );
            anyhow::ensure!(input.sample_valid, "insufficient_samples");
            let capacity = input
                .model
                .map(|m| catalog::resolve(input.catalog, m, input.plan, id, input.mode))
                .transpose()?
                .flatten()
                .context("no_model_budget")?;
            let capacity = Decimal::from_str(&capacity.api_equivalent_usd)?;
            let correction = div(delta / Decimal::from(100), normalized)?;
            anyhow::ensure!(
                correction > Decimal::ZERO && correction <= Decimal::from(1_000_000_000u64),
                "precision_limit"
            );
            let projected = mul(
                mul(div(input.rate, capacity)?, correction)?,
                Decimal::from(100),
            )?;
            anyhow::ensure!(projected > Decimal::ZERO, "zero_rate");
            let remaining = (Decimal::from(100) - Decimal::from_str(&point.used)?.max(high_water))
                .clamp(Decimal::ZERO, Decimal::from(100));
            let until = Decimal::from(point.reset - input.now);
            let lifetime = div(remaining, projected)?;
            let needed = div(remaining, until)?;
            let relative = mul(div(needed, projected)?, input.pace)?;
            anyhow::ensure!(
                relative <= Decimal::from(1_000_000_000u64),
                "precision_limit"
            );
            let target_usd = mul(
                mul(div(input.rate, input.pace)?, relative)?,
                Decimal::from(3600),
            )?;
            let target_requests = mul(
                mul(div(input.request_rate, input.pace)?, relative)?,
                Decimal::from(3600),
            )?;
            result["status"] = json!("ready");
            result["reason"] = Value::Null;
            result["calibrationFactor"] = json!(decimal(correction));
            result["modelPercentPerHour"] = json!(decimal(mul(projected, Decimal::from(3600))?));
            result["requiredPercentPerHour"] = json!(decimal(mul(needed, Decimal::from(3600))?));
            result["requiredIntensity"] = json!(decimal(relative));
            result["requiredUsdPerHour"] = json!(decimal(target_usd));
            result["requiredRequestsPerHour"] = json!(decimal(target_requests));
            result["coversReset"] = json!(lifetime >= until);
            result["runtimeSeconds"] = json!(lifetime
                .min(until)
                .round_dp(6)
                .ceil()
                .to_i64()
                .context("Calibration precision exceeded")?);
            result["equivalentModelCapacityUsd"] = json!(decimal(div(capacity, correction)?));
            Ok(result.clone())
        })();
        match estimates {
            Ok(value) => windows.push(value),
            Err(e) => {
                let text = e.to_string();
                result["reason"] = json!(if text.contains("precision") {
                    "precision_limit"
                } else {
                    text.as_str()
                });
                windows.push(result);
            }
        }
    }
    let target = if !windows.is_empty() && windows.iter().all(|w| w["status"] == "ready") {
        windows.iter().min_by(|a,b|Decimal::from_str(a["requiredIntensity"].as_str().unwrap()).unwrap().cmp(&Decimal::from_str(b["requiredIntensity"].as_str().unwrap()).unwrap())).map(|w|json!({"intensity":w["requiredIntensity"],"durationMinutes":w["durationMinutes"]}))
    } else {
        None
    };
    Ok(
        json!({"identityVerified":who.is_some(),"historyPoints":history.len(),"windows":windows,"currentWindowTarget":target}),
    )
}

#[cfg(test)]
mod tests;
