use crate::{
    model::*,
    query_store::{Row, Selection},
    store::Store,
};
use anyhow::{Context, Result};
use chrono::{Datelike, TimeZone};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::{collections::BTreeMap, str::FromStr};

struct SummaryValue<'a> {
    tokens: &'a Tokens,
    count: u64,
    amount: Option<&'a str>,
    status: Option<u16>,
    converted: bool,
    subscription: bool,
}
pub fn summary(records: &[UsageRecord]) -> Value {
    aggregate(records.iter().map(|r| {
        SummaryValue {
            tokens: &r.tokens,
            count: r.count,
            amount: r.cost.amount_usd.as_deref(),
            status: r.status_code,
            converted: r
                .cost
                .conversion
                .as_ref()
                .is_some_and(|c| c.units_per_usd.is_some()),
            subscription: r.cost.billing_basis.as_deref() == Some("subscription_usage"),
        }
    }))
}
pub(crate) fn summary_rows(records: &[Row]) -> Value {
    aggregate(records.iter().map(|r| SummaryValue {
        tokens: &r.tokens,
        count: r.count,
        amount: r.amount.as_deref(),
        status: r.status_code,
        converted: r.converted,
        subscription: r.billing_basis.as_deref() == Some("subscription_usage"),
    }))
}
fn aggregate<'a>(records: impl Iterator<Item = SummaryValue<'a>>) -> Value {
    let mut total = SummaryAccumulator::default();
    for r in records {
        total.add(r);
    }
    total.finish()
}

#[derive(Default)]
struct SummaryAccumulator {
    tokens: Tokens,
    count: u64,
    cost: Decimal,
    known: u64,
    success: u64,
    unpriced: u64,
    converted: bool,
    subscription: u64,
}
impl SummaryAccumulator {
    fn add_row(&mut self, r: &Row) {
        self.add(SummaryValue {
            tokens: &r.tokens,
            count: r.count,
            amount: r.amount.as_deref(),
            status: r.status_code,
            converted: r.converted,
            subscription: r.billing_basis.as_deref() == Some("subscription_usage"),
        });
    }
    fn add(&mut self, r: SummaryValue<'_>) {
        self.count += r.count;
        self.tokens.input += r.tokens.input;
        self.tokens.output += r.tokens.output;
        self.tokens.cache_read += r.tokens.cache_read;
        self.tokens.cache_write += r.tokens.cache_write;
        self.cost += r
            .amount
            .and_then(|s| Decimal::from_str(s).ok())
            .unwrap_or_default();
        self.converted |= r.converted;
        if r.subscription {
            self.subscription += r.count;
        }
        if let Some(status) = r.status {
            self.known += r.count;
            if (200..300).contains(&status) {
                self.success += r.count
            }
        }
        if r.amount.is_none() {
            self.unpriced += r.count
        }
    }
    fn finish(&self) -> Value {
        let Self {
            tokens: t,
            count,
            cost,
            known,
            success,
            unpriced,
            converted,
            subscription,
        } = self;
        let cacheable = t.input + t.cache_read + t.cache_write;
        json!({"totalRequests":count,"totalCost":cost.normalize().to_string(),"totalInputTokens":t.input,"totalOutputTokens":t.output,"totalCacheReadTokens":t.cache_read,"totalCacheCreationTokens":t.cache_write,"successRate":if *known>0{Some(*success as f64 / *known as f64*100.)}else{None},"realTotalTokens":t.total(),"cacheHitRate":if cacheable>0{t.cache_read as f64/cacheable as f64}else{0.},"unpricedCount":unpriced,"knownStatusCount":known,"subscriptionUsageCount":subscription,"costConverted":converted})
    }
}
fn provider(record: &UsageRecord) -> String {
    crate::providers::name(record)
}
#[derive(Default)]
struct TimingAccumulator {
    output: u64,
    duration: i64,
    speed_samples: u64,
    decode_output: u64,
    decode_duration: i64,
    decode_samples: u64,
    durations: usize,
    duration_sum: f64,
    firsts: Vec<i64>,
}
impl TimingAccumulator {
    fn add(&mut self, r: &Row) {
        if let Some(t) = r
            .timing
            .as_ref()
            .filter(|_| r.grain == "event" && r.record_source == "session")
        {
            self.durations += 1;
            self.duration_sum += t.duration_ms as f64;
            let speed = crate::timing::speed_estimate(&r.harness, t, r.tokens.output);
            if speed.eligible {
                let ms = speed.duration_ms.expect("eligible speed duration");
                if speed.basis == crate::timing::SpeedBasis::Decode {
                    self.decode_output = self.decode_output.saturating_add(r.tokens.output);
                    self.decode_duration = self.decode_duration.saturating_add(ms);
                    self.decode_samples += 1;
                } else {
                    self.output = self.output.saturating_add(r.tokens.output);
                    self.duration = self.duration.saturating_add(ms);
                    self.speed_samples += 1;
                }
            }
            if let Some(first) = t.first_token_ms {
                self.firsts.push(first);
            }
        }
    }
    fn finish(mut self) -> Value {
        self.firsts.sort_unstable();
        let n = self.firsts.len();
        let median = if n == 0 {
            None
        } else {
            Some((self.firsts[(n - 1) / 2] as f64 + self.firsts[n / 2] as f64) / 2.)
        };
        json!({"speedOutputTokens":0,"speedGenerationMs":0,"estDecodeSpeedOutputTokens":self.decode_output,"estDecodeSpeedGenerationMs":self.decode_duration,"estDecodeSpeedSampleCount":self.decode_samples,"estSpeedOutputTokens":self.output,"estSpeedDurationMs":self.duration,"estSpeedSampleCount":self.speed_samples,"firstTokenMedianMs":null,"estFirstTokenMedianMs":median,"estFirstTokenSampleCount":n,"timingSampleCount":self.durations,"avgLatencyMs":if self.durations == 0 {None}else{Some(self.duration_sum/self.durations as f64)}})
    }
}
fn log(r: &UsageRecord, timing: Option<&UsageTiming>) -> Value {
    let mut v = json!({"requestId":format!("{}|{}",r.device_id,r.id),"providerId":provider(r),"providerName":provider(r),"appType":r.harness,"model":r.model,"requestModel":r.model,"pricingModel":r.cost.pricing_model,"costMultiplier":"1","inputTokens":r.tokens.input,"outputTokens":r.tokens.output,"cacheReadTokens":r.tokens.cache_read,"cacheCreationTokens":r.tokens.cache_write,"inputCostUsd":"0","outputCostUsd":"0","cacheReadCostUsd":"0","cacheCreationCostUsd":"0","totalCostUsd":r.cost.amount_usd,"isStreaming":false,"latencyMs":null,"firstTokenMs":null,"durationMs":null,"statusCode":r.status_code,"createdAt":r.occurred_at,"dataSource":"harness_scope","deviceId":r.device_id,"costSource":r.cost.method,"priceSource":r.cost.price_source,"pricingSnapshot":r.cost.snapshot,"recordSource":r.record_source});
    v["reasoningTokens"] = json!(r.tokens.reasoning);
    if let Some(t) = timing.filter(|_| r.grain == "event" && r.record_source == "session") {
        v["latencyMs"] = json!(t.duration_ms);
        v["durationMs"] = json!(t.duration_ms);
        v["firstTokenMs"] = json!(t.first_token_ms);
        v["timingMethod"] = json!("estimated");
        v["timingSource"] = json!(t.duration_source);
        v["firstTokenSource"] = json!(t.first_token_source);
        v["estimatedSpeed"] = json!(crate::timing::speed_estimate(
            &r.harness,
            t,
            r.tokens.output
        ));
    }
    v["nativeProvider"] = json!(r.provider);
    v["requestMetadata"] = json!(r.request_metadata);
    v["billingBasis"] = json!(r.cost.billing_basis);
    v["costConverted"] = json!(r
        .cost
        .conversion
        .as_ref()
        .is_some_and(|c| c.units_per_usd.is_some()));
    v["costConversion"] = json!(r.cost.conversion);
    v["pricingContext"] = json!(r.cost.context);
    v["pricingRuleVersion"] = json!(r.cost.rule_version);
    if let Some(p) = &r.cost.snapshot {
        let rate = if p.currency == "USD" {
            Some(Decimal::ONE)
        } else {
            r.cost
                .conversion
                .as_ref()
                .and_then(|c| c.units_per_usd.as_deref())
                .and_then(|s| Decimal::from_str(s).ok())
        };
        if let (Some(rate), Ok(parts)) = (rate, crate::pricing::components(&r.tokens, p)) {
            for (key, amount) in [
                "inputCostUsd",
                "outputCostUsd",
                "cacheReadCostUsd",
                "cacheCreationCostUsd",
            ]
            .into_iter()
            .zip(parts)
            {
                v[key] = json!((amount / rate).normalize().to_string());
            }
        }
    }
    v
}

// CC Switch v4.0.15 uses range-relative hourly buckets and local calendar days,
// including zero buckets. See third_party/UPSTREAM.md for source provenance.
#[derive(Default)]
struct TrendAccumulator {
    summary: SummaryAccumulator,
    models: BTreeMap<String, SummaryAccumulator>,
}

fn trends(
    store: &Store,
    selection: &Selection,
    start: i64,
    end: i64,
    timezone: chrono_tz::Tz,
) -> Result<Value> {
    anyhow::ensure!(start <= end, "Start time must not exceed end time");
    let local = |ts| {
        timezone
            .timestamp_opt(ts, 0)
            .single()
            .context("Invalid trend timestamp")
    };
    let hourly = end.saturating_sub(start) <= 86_400;
    let start_day = local(start)?.date_naive();
    let end_day = local(end)?.date_naive();
    let count = if hourly {
        ((end - start + 3599) / 3600).max(1)
    } else {
        end_day.signed_duration_since(start_day).num_days() + 1
    };
    let mut groups: BTreeMap<i64, TrendAccumulator> = BTreeMap::new();
    selection.visit_rows(store, false, false, |r| {
        if !legacy_allowed(
            &r.grain,
            r.legacy_date.as_deref(),
            r.legacy_timezone.as_deref(),
            start,
            end,
            timezone,
        ) {
            return Ok(());
        }
        let bucket = if hourly {
            ((r.occurred_at - start) / 3600).min(count - 1)
        } else {
            // The complete-day guard uses a rollup's original timezone. Its
            // displayed bucket must still belong to the configured query zone.
            let day = local(r.occurred_at)?.date_naive();
            day.signed_duration_since(start_day).num_days()
        };
        if (0..count).contains(&bucket) {
            let group = groups.entry(bucket).or_default();
            group.summary.add_row(&r);
            group.models.entry(r.model.clone()).or_default().add_row(&r);
        }
        Ok(())
    })?;
    let mut rows = Vec::new();
    for bucket in 0..count {
        let date = if hourly {
            local(start + bucket * 3600)?.to_rfc3339()
        } else {
            let day = start_day
                .checked_add_signed(chrono::Duration::days(bucket))
                .context("Invalid trend day")?;
            // Some zones advance the clock at midnight. Use the first valid minute
            // of the day; a wholly skipped civil date has no time bucket.
            let first = (0..1440).find_map(|minute| {
                timezone
                    .from_local_datetime(&day.and_hms_opt(minute / 60, minute % 60, 0).unwrap())
                    .earliest()
            });
            let Some(first) = first else {
                continue;
            };
            first.to_rfc3339()
        };
        let group = groups.remove(&bucket).unwrap_or_default();
        let s = group.summary.finish();
        let model_breakdown: Vec<Value> = group.models
            .into_iter()
            .map(|(model, summary)| {
                let s = summary.finish();
                json!({"model":model,"requestCount":s["totalRequests"],"totalTokens":s["realTotalTokens"],"totalCost":s["totalCost"],"costConverted":s["costConverted"]})
            })
            .collect();
        rows.push(json!({"date":date,"requestCount":s["totalRequests"],"totalCost":s["totalCost"],"costConverted":s["costConverted"],"totalTokens":s["realTotalTokens"],"totalInputTokens":s["totalInputTokens"],"totalOutputTokens":s["totalOutputTokens"],"totalCacheCreationTokens":s["totalCacheCreationTokens"],"totalCacheReadTokens":s["totalCacheReadTokens"],"modelBreakdown":model_breakdown}));
    }
    Ok(json!(rows))
}

pub fn query(store: &Store, kind: &str, args: &Value) -> Result<Value> {
    let timezone: chrono_tz::Tz = store.config()?.timezone.parse()?;
    let (start, end) = if kind == "todaySummary" {
        let day = chrono::Utc::now().with_timezone(&timezone).date_naive();
        let midnight = |day: chrono::NaiveDate| {
            timezone
                .from_local_datetime(&day.and_hms_opt(0, 0, 0).unwrap())
                .earliest()
                .map(|t| t.timestamp())
                .ok_or_else(|| anyhow::anyhow!("Invalid local midnight"))
        };
        (midnight(day)?, midnight(day.succ_opt().unwrap())? - 1)
    } else if kind == "trends" {
        let end = args["endDate"]
            .as_i64()
            .unwrap_or_else(|| chrono::Utc::now().timestamp());
        (
            args["startDate"]
                .as_i64()
                .unwrap_or_else(|| end.saturating_sub(86_400)),
            end,
        )
    } else {
        (
            args["startDate"].as_i64().unwrap_or(0),
            args["endDate"].as_i64().unwrap_or(i64::MAX),
        )
    };
    let mut selection = Selection::new(store, args, start, end)?;
    if kind == "hasUsage" {
        let mut stmt = store.conn.prepare_cached(&format!(
            "SELECT u.grain,u.legacy_date,u.legacy_timezone FROM usage_index u WHERE {}",
            selection.sql
        ))?;
        let mut rows = stmt.query(rusqlite::params_from_iter(selection.params.iter()))?;
        while let Some(row) = rows.next()? {
            let grain: String = row.get(0)?;
            let date: Option<String> = row.get(1)?;
            let zone: Option<String> = row.get(2)?;
            if legacy_allowed(
                &grain,
                date.as_deref(),
                zone.as_deref(),
                start,
                end,
                timezone,
            ) {
                return Ok(json!(true));
            }
        }
        return Ok(json!(false));
    }
    if kind == "logs" {
        return logs(store, &selection, args);
    }
    if kind == "detail" {
        selection.equal(
            "u.device_id||'|'||u.id",
            args["requestId"].as_str().unwrap_or(""),
        );
        if let Some(device) = args["deviceId"].as_str() {
            selection.equal("u.device_id", device);
        }
        return detail(store, &selection);
    }
    if kind == "firstDate" {
        return first_date(store, &selection, start, end, timezone);
    }
    match kind {
        "models" | "providers" => {
            grouped_stats(store, &selection, kind == "models", start, end, timezone)
        }
        "trends" => trends(store, &selection, start, end, timezone),
        "summary" | "todaySummary" | "summaryByApp" | "sources" => {
            let mut groups: BTreeMap<String, SummaryAccumulator> = BTreeMap::new();
            if matches!(kind, "summary" | "todaySummary") {
                groups.insert(String::new(), SummaryAccumulator::default());
            }
            selection.visit_rows(store, false, false, |r| {
                if legacy_allowed(
                    &r.grain,
                    r.legacy_date.as_deref(),
                    r.legacy_timezone.as_deref(),
                    start,
                    end,
                    timezone,
                ) {
                    let key = match kind {
                        "summaryByApp" => r.harness.clone(),
                        "sources" => r.record_source.clone(),
                        _ => String::new(),
                    };
                    groups.entry(key).or_default().add_row(&r);
                }
                Ok(())
            })?;
            match kind {
                "summary" | "todaySummary" => Ok(groups.remove("").unwrap().finish()),
                "summaryByApp" => Ok(json!(groups.into_iter().map(|(app, summary)| json!({"appType":app,"summary":summary.finish()})).collect::<Vec<_>>())),
                "sources" => Ok(json!(groups.into_iter().map(|(source, summary)| { let s=summary.finish(); json!({"dataSource":source,"requestCount":s["totalRequests"],"totalCostUsd":s["totalCost"],"costConverted":s["costConverted"]}) }).collect::<Vec<_>>())),
                _ => unreachable!(),
            }
        }
        "heatmap" => {
            let mut values = Vec::new();
            selection.visit_rows(store, false, false, |r| {
                if legacy_allowed(&r.grain, r.legacy_date.as_deref(), r.legacy_timezone.as_deref(), start, end, timezone) {
                    if let Some(d) = timezone.timestamp_opt(r.occurred_at,0).single() {
                        values.push(json!({"date":d.format("%Y-%m-%d").to_string(),"weekday":d.weekday().num_days_from_monday(),"count":r.count}));
                    }
                }
                Ok(())
            })?;
            Ok(json!(values))
        }
        _ => anyhow::bail!("Unknown usage query: {kind}"),
    }
}

fn request_mode(row: &Row) -> String {
    // A GPT model name alone cannot establish a request's configured tier.
    row.request_metadata
        .as_ref()
        .filter(|m| {
            row.harness == "codex"
                && m.service_tier_source
                    .as_deref()
                    .is_some_and(|s| ["thread_settings", "turn_context"].contains(&s))
        })
        .and_then(|m| m.service_tier.as_deref())
        .map(|tier| match tier {
            "default" => "standard",
            "priority" | "fast" => "fast",
            other => other,
        })
        .unwrap_or("unknown")
        .into()
}

#[derive(Default)]
struct GroupAccumulator {
    summary: SummaryAccumulator,
    timing: TimingAccumulator,
    harness: Option<String>,
    mixed: bool,
}
impl GroupAccumulator {
    fn add(&mut self, row: &Row) {
        if let Some(first) = &self.harness {
            self.mixed |= first != &row.harness;
        } else {
            self.harness = Some(row.harness.clone());
        }
        self.summary.add_row(row);
        self.timing.add(row);
    }
}

fn grouped_stats(
    store: &Store,
    selection: &Selection,
    models: bool,
    start: i64,
    end: i64,
    timezone: chrono_tz::Tz,
) -> Result<Value> {
    let mut groups: BTreeMap<(String, Option<String>), GroupAccumulator> = BTreeMap::new();
    selection.visit_rows(store, true, models, |row| {
        if !legacy_allowed(
            &row.grain,
            row.legacy_date.as_deref(),
            row.legacy_timezone.as_deref(),
            start,
            end,
            timezone,
        ) {
            return Ok(());
        }
        let mode = (models && row.harness == "codex").then(|| request_mode(&row));
        let key = if models {
            row.model.clone()
        } else {
            row.provider_name()
        };
        groups.entry((key, mode)).or_default().add(&row);
        Ok(())
    })?;
    Ok(json!(groups.into_iter().map(|((key, mode), group)| {
        let s = group.summary.finish();
        let requests = s["totalRequests"].as_u64().unwrap_or(0);
        let mut row = json!({"model":key,"providerId":key,"providerName":key,
            "appType":if !group.mixed { group.harness.as_deref().unwrap_or("all") } else {"all"},
            "requestCount":requests,"totalTokens":s["realTotalTokens"],"totalCost":s["totalCost"],
            "costConverted":s["costConverted"],"successRate":s["successRate"],
            "avgCostPerRequest":if requests>0{(group.summary.cost/Decimal::from(requests)).normalize().to_string()}else{"0".into()}});
        if models {
            row["rowKey"] = json!(json!([key, mode]).to_string());
            row["requestMode"] = json!(mode);
        }
        row.as_object_mut().unwrap().extend(group.timing.finish().as_object().unwrap().clone());
        row
    }).collect::<Vec<_>>()))
}

fn legacy_allowed(
    grain: &str,
    date: Option<&str>,
    zone: Option<&str>,
    start: i64,
    end: i64,
    timezone: chrono_tz::Tz,
) -> bool {
    if grain != "legacy_day" {
        return true;
    }
    let Some(date) = date.and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
    else {
        return false;
    };
    let zone = zone
        .and_then(|z| z.parse::<chrono_tz::Tz>().ok())
        .unwrap_or(timezone);
    let begin = zone
        .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
        .earliest()
        .map(|d| d.timestamp());
    let next = date
        .succ_opt()
        .and_then(|day| {
            zone.from_local_datetime(&day.and_hms_opt(0, 0, 0).unwrap())
                .earliest()
        })
        .map(|d| d.timestamp());
    matches!((begin,next),(Some(b),Some(n)) if start<=b&&end>=n-1)
}
fn read_log(store: &Store, origin: &str, device: &str, id: &str) -> Result<Value> {
    use rusqlite::OptionalExtension;
    let table = if origin == "local" {
        "records"
    } else {
        "remote_records"
    };
    let payload: String = store
        .conn
        .prepare_cached(&format!(
            "SELECT payload FROM {table} WHERE device_id=?1 AND id=?2"
        ))?
        .query_row([device, id], |r| r.get(0))?;
    let record: UsageRecord = serde_json::from_str(&payload)?;
    let timing: Option<String> = store
        .conn
        .prepare_cached("SELECT payload FROM record_timings WHERE device_id=?1 AND id=?2")?
        .query_row([device, id], |r| r.get(0))
        .optional()?;
    let timing: Option<UsageTiming> = timing.map(|s| serde_json::from_str(&s)).transpose()?;
    Ok(log(&record, timing.as_ref()))
}
fn logs(store: &Store, selection: &Selection, args: &Value) -> Result<Value> {
    use rusqlite::{params_from_iter, types::Value as SqlValue};
    let condition = format!("{} AND u.grain='event'", selection.sql);
    let total: u64 = store
        .conn
        .prepare_cached(&format!(
            "SELECT COUNT(*) FROM usage_index u WHERE {condition}"
        ))?
        .query_row(params_from_iter(selection.params.iter()), |r| r.get(0))?;
    let page = args["page"].as_u64().unwrap_or(0);
    let size = args["pageSize"].as_u64().unwrap_or(20).clamp(1, 200);
    let mut params = selection.params.clone();
    params.extend([
        SqlValue::Integer(size as i64),
        SqlValue::Integer(page.saturating_mul(size).min(i64::MAX as u64) as i64),
    ]);
    let mut stmt=store.conn.prepare_cached(&format!("SELECT u.origin,u.device_id,u.id FROM usage_index u WHERE {condition} ORDER BY u.occurred_at DESC,u.id LIMIT ? OFFSET ?"))?;
    let keys = stmt
        .query_map(params_from_iter(params.iter()), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let rows = keys
        .iter()
        .map(|(origin, device, id)| read_log(store, origin, device, id))
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({"data":rows,"total":total,"page":page,"pageSize":size}))
}
fn detail(store: &Store, selection: &Selection) -> Result<Value> {
    use rusqlite::{params_from_iter, OptionalExtension};
    let key = store
        .conn
        .prepare_cached(&format!(
            "SELECT u.origin,u.device_id,u.id FROM usage_index u WHERE {} LIMIT 1",
            selection.sql
        ))?
        .query_row(params_from_iter(selection.params.iter()), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .optional()?;
    key.map(|(origin, device, id)| read_log(store, &origin, &device, &id))
        .transpose()
        .map(|v| v.unwrap_or(Value::Null))
}
fn first_date(
    store: &Store,
    selection: &Selection,
    start: i64,
    end: i64,
    timezone: chrono_tz::Tz,
) -> Result<Value> {
    use rusqlite::params_from_iter;
    let mut stmt=store.conn.prepare_cached(&format!("SELECT u.occurred_at,u.grain,u.legacy_date,u.legacy_timezone FROM usage_index u WHERE {} ORDER BY u.occurred_at",selection.sql))?;
    let mut rows = stmt.query(params_from_iter(selection.params.iter()))?;
    while let Some(row) = rows.next()? {
        let grain: String = row.get(1)?;
        let date: Option<String> = row.get(2)?;
        let zone: Option<String> = row.get(3)?;
        if legacy_allowed(
            &grain,
            date.as_deref(),
            zone.as_deref(),
            start,
            end,
            timezone,
        ) {
            if let Some(time) = timezone.timestamp_opt(row.get(0)?, 0).single() {
                return Ok(json!(time.format("%Y-%m-%d").to_string()));
            }
        }
    }
    Ok(Value::Null)
}
