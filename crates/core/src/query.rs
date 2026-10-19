use crate::{model::*, store::Store};
use anyhow::{Context, Result};
use chrono::{Datelike, TimeZone};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::{collections::BTreeMap, str::FromStr};

fn amount(r: &UsageRecord) -> Decimal {
    r.cost
        .amount_usd
        .as_deref()
        .and_then(|s| Decimal::from_str(s).ok())
        .unwrap_or_default()
}
pub fn summary(records: &[UsageRecord]) -> Value {
    let mut t = Tokens::default();
    let mut count = 0;
    let mut cost = Decimal::ZERO;
    let mut known = 0;
    let mut success = 0;
    let mut unpriced = 0;
    for r in records {
        count += r.count;
        t.input += r.tokens.input;
        t.output += r.tokens.output;
        t.cache_read += r.tokens.cache_read;
        t.cache_write += r.tokens.cache_write;
        cost += amount(r);
        if let Some(status) = r.status_code {
            known += r.count;
            if (200..300).contains(&status) {
                success += r.count
            }
        }
        if r.cost.amount_usd.is_none() {
            unpriced += r.count
        }
    }
    let cacheable = t.input + t.cache_read + t.cache_write;
    json!({"totalRequests":count,"totalCost":cost.normalize().to_string(),"totalInputTokens":t.input,"totalOutputTokens":t.output,"totalCacheReadTokens":t.cache_read,"totalCacheCreationTokens":t.cache_write,"successRate":if known>0{Some(success as f64/known as f64*100.)}else{None},"realTotalTokens":t.total(),"cacheHitRate":if cacheable>0{t.cache_read as f64/cacheable as f64}else{0.},"unpricedCount":unpriced,"knownStatusCount":known,"subscriptionUsageCount":records.iter().filter(|r|r.cost.billing_basis.as_deref()==Some("subscription_usage")).map(|r|r.count).sum::<u64>(),"costConverted":records.iter().any(|r|r.cost.conversion.as_ref().is_some_and(|c|c.units_per_usd.is_some()))})
}
fn provider(record: &UsageRecord) -> String {
    crate::providers::name(record)
}
type Timings = BTreeMap<(String, String), UsageTiming>;
fn timing_for<'a>(r: &UsageRecord, timings: &'a Timings) -> Option<&'a UsageTiming> {
    if r.grain != "event" || r.record_source != "session" {
        return None;
    }
    timings.get(&(r.device_id.clone(), r.id.clone()))
}
fn timing_stats(records: &[UsageRecord], timings: &Timings) -> Value {
    let mut output = 0u64;
    let mut duration = 0i64;
    let mut speed_samples = 0u64;
    let mut durations = Vec::new();
    let mut firsts = Vec::new();
    for r in records {
        if let Some(t) = timing_for(r, timings) {
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
fn log(r: &UsageRecord, timings: &Timings) -> Value {
    let mut v = json!({"requestId":format!("{}|{}",r.device_id,r.id),"providerId":provider(r),"providerName":provider(r),"appType":r.harness,"model":r.model,"requestModel":r.model,"pricingModel":r.cost.pricing_model,"costMultiplier":"1","inputTokens":r.tokens.input,"outputTokens":r.tokens.output,"cacheReadTokens":r.tokens.cache_read,"cacheCreationTokens":r.tokens.cache_write,"inputCostUsd":"0","outputCostUsd":"0","cacheReadCostUsd":"0","cacheCreationCostUsd":"0","totalCostUsd":r.cost.amount_usd,"isStreaming":false,"latencyMs":null,"firstTokenMs":null,"durationMs":null,"statusCode":r.status_code,"createdAt":r.occurred_at,"dataSource":"harness_scope","deviceId":r.device_id,"costSource":r.cost.method,"priceSource":r.cost.price_source,"pricingSnapshot":r.cost.snapshot,"recordSource":r.record_source});
    if let Some(t) = timing_for(r, timings) {
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
fn trends(
    records: Vec<UsageRecord>,
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
    let mut groups: BTreeMap<i64, Vec<UsageRecord>> = BTreeMap::new();
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
        let s = summary(&records);
        rows.push(json!({"date":date,"requestCount":s["totalRequests"],"totalCost":s["totalCost"],"costConverted":s["costConverted"],"totalTokens":s["realTotalTokens"],"totalInputTokens":s["totalInputTokens"],"totalOutputTokens":s["totalOutputTokens"],"totalCacheCreationTokens":s["totalCacheCreationTokens"],"totalCacheReadTokens":s["totalCacheReadTokens"]}));
    }
    Ok(json!(rows))
}

pub fn query(store: &Store, kind: &str, args: &Value) -> Result<Value> {
    let scope = args["scope"].as_str().unwrap_or("local");
    let records = store.records(scope)?;
    let timings = store.timings()?;
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
    let filter = args.get("filters").unwrap_or(args);
    let selected: Vec<UsageRecord> = records
        .into_iter()
        .filter(|r| {
            r.occurred_at >= start
                && r.occurred_at <= end
                && filter["appType"]
                    .as_str()
                    .is_none_or(|s| s == "all" || r.harness == s)
                && filter["model"].as_str().is_none_or(|s| r.model == s)
                && filter["providerName"]
                    .as_str()
                    .is_none_or(|s| provider(r) == s || r.provider.is_none() && r.harness == s)
                && filter["statusCode"]
                    .as_u64()
                    .is_none_or(|s| r.status_code == Some(s as u16))
                && args["sessionId"]
                    .as_str()
                    .is_none_or(|s| r.session_id.as_deref() == Some(s))
        })
        .filter(|r| {
            if r.grain != "legacy_day" {
                return true;
            }
            let Some(date) = r
                .legacy_date
                .as_deref()
                .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
            else {
                return false;
            };
            let zone = r
                .legacy_timezone
                .as_deref()
                .and_then(|z| z.parse::<chrono_tz::Tz>().ok())
                .unwrap_or(timezone);
            let begin = zone
                .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
                .earliest()
                .map(|d| d.timestamp());
            let next = zone
                .from_local_datetime(&date.succ_opt().unwrap().and_hms_opt(0, 0, 0).unwrap())
                .earliest()
                .map(|d| d.timestamp());
            matches!((begin,next),(Some(b),Some(n)) if start<=b&&end>=n-1)
        })
        .collect();
    match kind{
        "summary"|"todaySummary"=>Ok(summary(&selected)),
        "summaryByApp"=>{let mut groups:BTreeMap<String,Vec<UsageRecord>>=BTreeMap::new();for r in selected{groups.entry(r.harness.clone()).or_default().push(r)}Ok(json!(groups.into_iter().map(|(app,rs)|json!({"appType":app,"summary":summary(&rs)})).collect::<Vec<_>>()))},
        "firstDate"=>Ok(json!(selected.iter().filter_map(|r|timezone.timestamp_opt(r.occurred_at,0).single()).min().map(|d|d.format("%Y-%m-%d").to_string()))),
        "logs"=>{let mut rows:Vec<_>=selected.into_iter().filter(|r|r.grain=="event").collect();rows.sort_by(|a,b|b.occurred_at.cmp(&a.occurred_at).then(a.id.cmp(&b.id)));let total=rows.len();let page=args["page"].as_u64().unwrap_or(0) as usize;let size=args["pageSize"].as_u64().unwrap_or(20).clamp(1,200) as usize;Ok(json!({"data":rows.iter().skip(page.saturating_mul(size)).take(size).map(|r|log(r,&timings)).collect::<Vec<_>>(),"total":total,"page":page,"pageSize":size}))},
        "detail"=>Ok(selected.iter().find(|r|format!("{}|{}",r.device_id,r.id)==args["requestId"].as_str().unwrap_or("")&&args["deviceId"].as_str().is_none_or(|id|r.device_id==id)).map(|r|log(r,&timings)).unwrap_or(Value::Null)),
        "models"|"providers"=>{let mut groups:BTreeMap<String,Vec<UsageRecord>>=BTreeMap::new();for r in selected{groups.entry(if kind=="models"{r.model.clone()}else{provider(&r)}).or_default().push(r)}Ok(json!(groups.into_iter().map(|(key,rs)|{let s=summary(&rs);let mut row=json!({"model":key,"providerId":key,"providerName":key,"appType":if rs.iter().all(|r|r.harness==rs[0].harness){rs[0].harness.as_str()}else{"all"},"requestCount":s["totalRequests"],"totalTokens":s["realTotalTokens"],"totalCost":s["totalCost"],"costConverted":s["costConverted"],"successRate":s["successRate"],"avgLatencyMs":0,"avgCostPerRequest":if s["totalRequests"].as_u64().unwrap_or(0)>0{(rs.iter().map(amount).sum::<Decimal>()/Decimal::from(s["totalRequests"].as_u64().unwrap())).normalize().to_string()}else{"0".into()}});row.as_object_mut().unwrap().extend(timing_stats(&rs,&timings).as_object().unwrap().clone());row}).collect::<Vec<_>>()))},
        "trends"=>trends(selected,start,end,timezone),
        "sources"=>{let mut groups:BTreeMap<String,Vec<UsageRecord>>=BTreeMap::new();for r in selected{groups.entry(r.record_source.clone()).or_default().push(r)}Ok(json!(groups.into_iter().map(|(source,rs)|{let s=summary(&rs);json!({"dataSource":source,"requestCount":s["totalRequests"],"totalCostUsd":s["totalCost"],"costConverted":s["costConverted"]})}).collect::<Vec<_>>()))},
        "heatmap"=>Ok(json!(selected.iter().filter_map(|r|timezone.timestamp_opt(r.occurred_at,0).single().map(|d|json!({"date":d.format("%Y-%m-%d").to_string(),"weekday":d.weekday().num_days_from_monday(),"count":r.count}))).collect::<Vec<_>>())),
        _=>anyhow::bail!("Unknown usage query: {kind}")
    }
}
