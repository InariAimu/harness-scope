//! Two actual quota observations and concurrent linked requests; never changes billing facts.
use super::*;
use crate::{model::UsageRecord, pricing};
use rusqlite::{params, params_from_iter};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Query {
    id: String,
    first_sequence: i64,
    last_sequence: i64,
    bucket_id: Option<String>,
    window_id: String,
}

fn number(value: &Value) -> Option<Decimal> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.is_number().then(|| value.to_string()))
        .and_then(|s| Decimal::from_str(&s).ok())
}
fn decimal(value: Decimal) -> String {
    value.round_dp(12).normalize().to_string()
}
fn window<'a>(sample: &'a Value, query: &Query) -> Option<&'a Value> {
    let matches: Vec<_> = sample["windows"]
        .as_array()?
        .iter()
        .filter(|w| {
            w["windowId"] == query.window_id && w["bucketId"].as_str() == query.bucket_id.as_deref()
        })
        .collect();
    (matches.len() == 1).then(|| matches[0])
}
fn group(sample: &Value, w: &Value) -> Value {
    let custom_item = sample["data"]["items"]
        .as_array()
        .and_then(|items| items.iter().find(|i| i["id"] == w["windowId"]));
    let custom_definition = custom_item.map(|i| json!([i["unit"], i["total"], i["planName"]]));
    let bucket_plan = sample["data"]["buckets"]
        .as_array()
        .and_then(|b| b.iter().find(|b| b["id"] == w["bucketId"]))
        .map(|b| b["planType"].clone());
    json!([
        sample["provider"],
        sample["accountRevision"],
        sample["data"]["identity"],
        sample["data"]["identitySource"],
        sample["data"]["planType"],
        bucket_plan,
        custom_definition,
        w["bucketId"],
        w["windowId"],
        w["durationMinutes"],
        w["resetsAt"]
    ])
}

pub(super) fn estimate(engine: &Engine, args: Value) -> Result<Value> {
    let query: Query = serde_json::from_value(args)?;
    uuid::Uuid::parse_str(&query.id).context("Invalid account ID")?;
    anyhow::ensure!(
        query.first_sequence > 0 && query.last_sequence > query.first_sequence,
        "Choose two ordered observations"
    );
    let store = engine.read_lock()?;
    store.transaction(|| calculate(&store, &query))
}

fn calculate(store: &Store, query: &Query) -> Result<Value> {
    let item = load(store)?
        .into_iter()
        .find(|i| i.account.id == query.id)
        .context("Provider account no longer exists")?;
    let get = |seq| -> Result<Value> {
        let payload: String = store
            .conn
            .query_row(
                "SELECT payload FROM provider_account_observations WHERE account_id=?1 AND seq=?2",
                params![query.id, seq],
                |r| r.get(0),
            )
            .context("Observation no longer exists")?;
        Ok(serde_json::from_str(&payload)?)
    };
    let first = get(query.first_sequence)?;
    let last = get(query.last_sequence)?;
    let now = Utc::now().timestamp();
    let mut result = json!({"status":"unavailable","reason":null,"basis":"observedMix","firstSequence":query.first_sequence,
        "lastSequence":query.last_sequence,"sampleStartMs":first["observedAtMs"],"sampleEndMs":last["observedAtMs"],
        "pricingAt":now,"requests":0,"unpricedRequests":0,"modelCount":0,"models":[]});
    let outcome = (|| -> Result<()> {
        let start = first["observedAtMs"].as_i64().context("invalid_time")?;
        let end = last["observedAtMs"].as_i64().context("invalid_time")?;
        anyhow::ensure!(
            end - start >= 60_000 && end <= now * 1000 + 999,
            "insufficient_span"
        );
        anyhow::ensure!(last["accountRevision"] == item.revision, "account_changed");
        let a = window(&first, query).context("missing_window")?;
        let b = window(&last, query).context("missing_window")?;
        let expected = group(&first, a);
        anyhow::ensure!(group(&last, b) == expected, "different_window");
        if item.account.provider == "openai-codex" {
            let identity = last["data"]["identity"].as_str().unwrap_or("");
            anyhow::ensure!(
                last["data"]["identitySource"] == "accountId"
                    && identity.len() == 64
                    && identity.bytes().all(|b| b.is_ascii_hexdigit()),
                "unverified_identity"
            );
        }
        if custom::is_provider(&item.account.provider) {
            let custom_item = last["data"]["items"]
                .as_array()
                .and_then(|items| items.iter().find(|i| i["id"] == query.window_id))
                .context("missing_window")?;
            anyhow::ensure!(custom_item["windowType"] == "fixed", "unsupported_window");
        }
        let reset = b["resetsAt"].as_i64().context("missing_reset")?;
        anyhow::ensure!(reset > end / 1000, "expired_sample");
        let mut stmt = store.conn.prepare_cached("SELECT payload FROM provider_account_observations WHERE account_id=?1 AND seq>=?2 AND seq<=?3 ORDER BY seq")?;
        let samples = stmt.query_map(
            params![query.id, query.first_sequence, query.last_sequence],
            |r| r.get::<_, String>(0),
        )?;
        let mut previous = None;
        let mut previous_at = None;
        for sample in samples {
            let sample: Value = serde_json::from_str(&sample?)?;
            if custom::is_provider(&item.account.provider) {
                let definition = sample["data"]["items"]
                    .as_array()
                    .and_then(|items| items.iter().find(|i| i["id"] == query.window_id))
                    .context("missing_window")?;
                anyhow::ensure!(definition["windowType"] == "fixed", "unsupported_window");
            }
            let w = window(&sample, query).context("missing_window")?;
            anyhow::ensure!(group(&sample, w) == expected, "different_window");
            let at = sample["observedAtMs"].as_i64().context("invalid_time")?;
            anyhow::ensure!(
                previous_at.is_none_or(|p| at >= p) && at >= start && at <= end,
                "invalid_time"
            );
            anyhow::ensure!(
                w["status"] == "ok" || w["status"] == "rate-limited",
                "unknown_quota"
            );
            anyhow::ensure!(
                w["startsAt"].as_i64().is_none_or(|s| s <= at / 1000) && reset > at / 1000,
                "expired_sample"
            );
            let used = number(&w["usedPercent"]).context("unknown_quota")?;
            anyhow::ensure!(
                used >= Decimal::ZERO && used <= Decimal::from(100),
                "quota_out_of_range"
            );
            anyhow::ensure!(previous.is_none_or(|p| used >= p), "quota_decreased");
            previous = Some(used);
            previous_at = Some(at);
        }
        let delta = number(&b["usedPercent"]).context("unknown_quota")?
            - number(&a["usedPercent"]).context("unknown_quota")?;
        result["deltaPercent"] = json!(decimal(delta));
        anyhow::ensure!(delta >= Decimal::ONE, "insufficient_delta");
        anyhow::ensure!(!item.account.links.is_empty(), "no_links");
        let selected = linked_selection(store, &item.account, start / 1000, end / 1000)?;
        let mut stmt = store.conn.prepare_cached(&format!("SELECT CASE WHEN u.origin='local' THEN r.payload ELSE remote.payload END FROM usage_index u LEFT JOIN records r ON u.origin='local' AND r.device_id=u.device_id AND r.id=u.id LEFT JOIN remote_records remote ON u.origin='remote' AND remote.device_id=u.device_id AND remote.id=u.id WHERE {} AND u.occurred_at>?",selected.sql))?;
        let mut parameters = selected.params.clone();
        parameters.push((start / 1000).into());
        let rows = stmt.query_map(params_from_iter(parameters.iter()), |r| {
            r.get::<_, String>(0)
        })?;
        let prices = store.prices()?;
        let settings = store.pricing_settings()?;
        let go = (item.account.provider == "opencode-go")
            .then(|| pricing::go_catalog::load(store))
            .transpose()?;
        let codex = (item.account.provider == "openai-codex")
            .then(|| codex::catalog::load(store))
            .transpose()?;
        if let Some(catalog) = &codex {
            anyhow::ensure!(
                query
                    .bucket_id
                    .as_ref()
                    .is_some_and(|id| catalog.bucket_ids.contains(id)),
                "unsupported_bucket"
            );
        }
        let mut total = Decimal::ZERO;
        let mut count = 0u64;
        let mut unpriced = 0u64;
        let mut models: BTreeMap<String, (u64, Decimal)> = BTreeMap::new();
        for row in rows {
            let row: UsageRecord = serde_json::from_str(&row?)?;
            // Daily imports cannot establish which side of either refresh they belong to.
            anyhow::ensure!(row.grain == "event", "legacy_usage");
            count = count.saturating_add(row.count);
            let quoted = if let Some(catalog) = &go {
                pricing::go_catalog::find(catalog, &row.model).and_then(|m| {
                    pricing::go_catalog::quote(
                        m,
                        &catalog.peak_schedule,
                        &row.tokens,
                        row.occurred_at,
                        None,
                    )
                    .ok()
                    .map(|q| q.0)
                })
            } else {
                let provider = if codex.is_some() {
                    Some("openai")
                } else {
                    row.provider.as_deref()
                };
                let model = codex
                    .as_ref()
                    .and_then(|c| codex::catalog::find(c, &row.model));
                pricing::find_for_provider(&prices, &row.model, provider)
                    .or_else(|| pricing::find(&prices, &row.model))
                    .or_else(|| {
                        model.and_then(|m| {
                            pricing::find_for_provider(&prices, &m.id, provider)
                                .or_else(|| pricing::find(&prices, &m.id))
                        })
                    })
                    .and_then(|p| {
                        pricing::estimate_at(&row.tokens, p, row.occurred_at, &settings).ok()
                    })
                    .and_then(|c| c.amount_usd)
                    .and_then(|s| Decimal::from_str(&s).ok())
            };
            let model = models.entry(row.model).or_default();
            model.0 = model.0.saturating_add(row.count);
            match quoted {
                Some(cost) => {
                    total = total.checked_add(cost).context("precision_limit")?;
                    model.1 = model.1.checked_add(cost).context("precision_limit")?;
                }
                None => unpriced = unpriced.saturating_add(row.count),
            }
        }
        result["requests"] = json!(count);
        result["unpricedRequests"] = json!(unpriced);
        result["linkedUsd"] = json!(decimal(total));
        result["modelCount"] = json!(models.len());
        result["models"] = json!(models.into_iter().map(|(model,(requests,cost))|json!({"model":model,"requests":requests,"costUsd":decimal(cost)})).collect::<Vec<_>>());
        anyhow::ensure!(unpriced == 0, "unpriced_usage");
        anyhow::ensure!(count > 0 && total > Decimal::ZERO, "no_priced_usage");
        let capacity = total
            .checked_mul(Decimal::from(100))
            .and_then(|v| v.checked_div(delta))
            .context("precision_limit")?;
        let remaining = number(&b["remainingPercent"]).context("unknown_quota")?;
        result["totalEquivalentUsd"] = json!(decimal(capacity));
        result["remainingEquivalentUsd"] = json!(decimal(
            capacity
                .checked_mul(remaining)
                .and_then(|v| v.checked_div(Decimal::from(100)))
                .context("precision_limit")?
        ));
        result["status"] = json!("ready");
        Ok(())
    })();
    if let Err(error) = outcome {
        result["reason"] = json!(error.to_string());
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
