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

fn amount(r: &Row) -> Decimal {
    r.amount
        .as_deref()
        .and_then(|s| Decimal::from_str(s).ok())
        .unwrap_or_default()
}
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
fn summary_rows(records: &[Row]) -> Value {
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
    let mut t = Tokens::default();
    let mut count = 0;
    let mut cost = Decimal::ZERO;
    let mut known = 0;
    let mut success = 0;
    let mut unpriced = 0;
    let mut converted = false;
    let mut subscription = 0;
    for r in records {
        count += r.count;
        t.input += r.tokens.input;
        t.output += r.tokens.output;
        t.cache_read += r.tokens.cache_read;
        t.cache_write += r.tokens.cache_write;
        cost += r
            .amount
            .and_then(|s| Decimal::from_str(s).ok())
            .unwrap_or_default();
        converted |= r.converted;
        if r.subscription {
            subscription += r.count;
        }
        if let Some(status) = r.status {
            known += r.count;
            if (200..300).contains(&status) {
                success += r.count
            }
        }
        if r.amount.is_none() {
            unpriced += r.count
        }
    }
    let cacheable = t.input + t.cache_read + t.cache_write;
    json!({"totalRequests":count,"totalCost":cost.normalize().to_string(),"totalInputTokens":t.input,"totalOutputTokens":t.output,"totalCacheReadTokens":t.cache_read,"totalCacheCreationTokens":t.cache_write,"successRate":if known>0{Some(success as f64/known as f64*100.)}else{None},"realTotalTokens":t.total(),"cacheHitRate":if cacheable>0{t.cache_read as f64/cacheable as f64}else{0.},"unpricedCount":unpriced,"knownStatusCount":known,"subscriptionUsageCount":subscription,"costConverted":converted})
}
fn provider(record: &UsageRecord) -> String {
    crate::providers::name(record)
}
fn timing_stats(records: &[Row]) -> Value {
    let mut output = 0u64;
    let mut duration = 0i64;
    let mut speed_samples = 0u64;
    let mut durations = Vec::new();
    let mut firsts = Vec::new();
    for r in records {
        if let Some(t) = r
            .timing
            .as_ref()
            .filter(|_| r.grain == "event" && r.record_source == "session")
        {
            durations.push(t.duration_ms);
            if r.tokens.output >= 200 && t.duration_ms >= 1000 {
                output = output.saturating_add(r.tokens.output);
                duration = duration.saturating_add(t.duration_ms);
                speed_samples += 1;
            }
            if let Some(first) = t.first_token_ms {
                firsts.push(first);
            }
        }
    }
    firsts.sort_unstable();
    let n = firsts.len();
    let median = if n == 0 {
        None
    } else {
        Some((firsts[(n - 1) / 2] as f64 + firsts[n / 2] as f64) / 2.)
    };
    json!({"speedOutputTokens":0,"speedGenerationMs":0,"estSpeedOutputTokens":output,"estSpeedDurationMs":duration,"estSpeedSampleCount":speed_samples,"firstTokenMedianMs":null,"estFirstTokenMedianMs":median,"estFirstTokenSampleCount":n,"timingSampleCount":durations.len(),"avgLatencyMs":if durations.is_empty(){None}else{Some(durations.iter().map(|v|*v as f64).sum::<f64>()/durations.len() as f64)}})
}
fn log(r: &UsageRecord, timing: Option<&UsageTiming>) -> Value {
    let mut v = json!({"requestId":format!("{}|{}",r.device_id,r.id),"providerId":provider(r),"providerName":provider(r),"appType":r.harness,"model":r.model,"requestModel":r.model,"pricingModel":r.cost.pricing_model,"costMultiplier":"1","inputTokens":r.tokens.input,"outputTokens":r.tokens.output,"cacheReadTokens":r.tokens.cache_read,"cacheCreationTokens":r.tokens.cache_write,"inputCostUsd":"0","outputCostUsd":"0","cacheReadCostUsd":"0","cacheCreationCostUsd":"0","totalCostUsd":r.cost.amount_usd,"isStreaming":false,"latencyMs":null,"firstTokenMs":null,"durationMs":null,"statusCode":r.status_code,"createdAt":r.occurred_at,"dataSource":"harness_scope","deviceId":r.device_id,"costSource":r.cost.method,"priceSource":r.cost.price_source,"pricingSnapshot":r.cost.snapshot,"recordSource":r.record_source});
    if let Some(t) = timing.filter(|_| r.grain == "event" && r.record_source == "session") {
        v["latencyMs"] = json!(t.duration_ms);
        v["durationMs"] = json!(t.duration_ms);
        v["firstTokenMs"] = json!(t.first_token_ms);
        v["timingMethod"] = json!("estimated");
        v["timingSource"] = json!(t.duration_source);
        v["firstTokenSource"] = json!(t.first_token_source);
    }
    v["nativeProvider"] = json!(r.provider);
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
fn trends(records: Vec<Row>, start: i64, end: i64, timezone: chrono_tz::Tz) -> Result<Value> {
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
    let mut groups: BTreeMap<i64, Vec<Row>> = BTreeMap::new();
    for r in records {
        let bucket = if hourly {
            ((r.occurred_at - start) / 3600).min(count - 1)
        } else {
            // The complete-day guard uses a rollup's original timezone. Its
            // displayed bucket must still belong to the configured query zone.
            let day = local(r.occurred_at)?.date_naive();
            day.signed_duration_since(start_day).num_days()
        };
        if (0..count).contains(&bucket) {
            groups.entry(bucket).or_default().push(r);
        }
    }
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
        let records = groups.remove(&bucket).unwrap_or_default();
        let s = summary_rows(&records);
        rows.push(json!({"date":date,"requestCount":s["totalRequests"],"totalCost":s["totalCost"],"costConverted":s["costConverted"],"totalTokens":s["realTotalTokens"],"totalInputTokens":s["totalInputTokens"],"totalOutputTokens":s["totalOutputTokens"],"totalCacheCreationTokens":s["totalCacheCreationTokens"],"totalCacheReadTokens":s["totalCacheReadTokens"]}));
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
    let selected: Vec<Row> = selection
        .rows(store, matches!(kind, "models" | "providers"))?
        .into_iter()
        .filter(|r| {
            legacy_allowed(
                &r.grain,
                r.legacy_date.as_deref(),
                r.legacy_timezone.as_deref(),
                start,
                end,
                timezone,
            )
        })
        .collect();
    match kind{
        "summary"|"todaySummary"=>Ok(summary_rows(&selected)),
        "summaryByApp"=>{let mut groups:BTreeMap<String,Vec<Row>>=BTreeMap::new();for r in selected{groups.entry(r.harness.clone()).or_default().push(r)}Ok(json!(groups.into_iter().map(|(app,rs)|json!({"appType":app,"summary":summary_rows(&rs)})).collect::<Vec<_>>()))},
        "models"|"providers"=>{let mut groups:BTreeMap<String,Vec<Row>>=BTreeMap::new();for r in selected{groups.entry(if kind=="models"{r.model.clone()}else{r.provider_name()}).or_default().push(r)}Ok(json!(groups.into_iter().map(|(key,rs)|{let s=summary_rows(&rs);let mut row=json!({"model":key,"providerId":key,"providerName":key,"appType":if rs.iter().all(|r|r.harness==rs[0].harness){rs[0].harness.as_str()}else{"all"},"requestCount":s["totalRequests"],"totalTokens":s["realTotalTokens"],"totalCost":s["totalCost"],"costConverted":s["costConverted"],"successRate":s["successRate"],"avgLatencyMs":0,"avgCostPerRequest":if s["totalRequests"].as_u64().unwrap_or(0)>0{(rs.iter().map(amount).sum::<Decimal>()/Decimal::from(s["totalRequests"].as_u64().unwrap())).normalize().to_string()}else{"0".into()}});row.as_object_mut().unwrap().extend(timing_stats(&rs).as_object().unwrap().clone());row}).collect::<Vec<_>>()))},
        "trends"=>trends(selected,start,end,timezone),
        "sources"=>{let mut groups:BTreeMap<String,Vec<Row>>=BTreeMap::new();for r in selected{groups.entry(r.record_source.clone()).or_default().push(r)}Ok(json!(groups.into_iter().map(|(source,rs)|{let s=summary_rows(&rs);json!({"dataSource":source,"requestCount":s["totalRequests"],"totalCostUsd":s["totalCost"],"costConverted":s["costConverted"]})}).collect::<Vec<_>>()))},
        "heatmap"=>Ok(json!(selected.iter().filter_map(|r|timezone.timestamp_opt(r.occurred_at,0).single().map(|d|json!({"date":d.format("%Y-%m-%d").to_string(),"weekday":d.weekday().num_days_from_monday(),"count":r.count}))).collect::<Vec<_>>())),
        _=>anyhow::bail!("Unknown usage query: {kind}")
    }
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
