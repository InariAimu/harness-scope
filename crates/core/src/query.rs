use crate::{model::*, store::Store};
use anyhow::Result;
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
    json!({"totalRequests":count,"totalCost":cost.normalize().to_string(),"totalInputTokens":t.input,"totalOutputTokens":t.output,"totalCacheReadTokens":t.cache_read,"totalCacheCreationTokens":t.cache_write,"successRate":if known>0{Some(success as f64/known as f64*100.)}else{None},"realTotalTokens":t.total(),"cacheHitRate":if cacheable>0{t.cache_read as f64/cacheable as f64}else{0.},"unpricedCount":unpriced,"knownStatusCount":known})
}
fn provider(harness: &str) -> String {
    format!("{harness} (Session)")
}
fn log(r: &UsageRecord) -> Value {
    let mut v = json!({"requestId":format!("{}|{}",r.device_id,r.id),"providerId":r.harness,"providerName":provider(&r.harness),"appType":r.harness,"model":r.model,"requestModel":r.model,"pricingModel":r.cost.pricing_model,"costMultiplier":"1","inputTokens":r.tokens.input,"outputTokens":r.tokens.output,"cacheReadTokens":r.tokens.cache_read,"cacheCreationTokens":r.tokens.cache_write,"inputCostUsd":"0","outputCostUsd":"0","cacheReadCostUsd":"0","cacheCreationCostUsd":"0","totalCostUsd":r.cost.amount_usd,"isStreaming":false,"latencyMs":0,"statusCode":r.status_code,"createdAt":r.occurred_at,"dataSource":"harness_scope","deviceId":r.device_id,"costSource":r.cost.method,"priceSource":r.cost.price_source,"pricingSnapshot":r.cost.snapshot,"recordSource":r.record_source});
    if let Some(p) = &r.cost.snapshot {
        let unit = |s: &str| Decimal::from_str(s).unwrap_or_default() / Decimal::from(1_000_000);
        v["inputCostUsd"] = json!((Decimal::from(r.tokens.input)
            * unit(&p.input_cost_per_million))
        .normalize()
        .to_string());
        v["outputCostUsd"] = json!((Decimal::from(r.tokens.output)
            * unit(&p.output_cost_per_million))
        .normalize()
        .to_string());
        v["cacheReadCostUsd"] = json!((Decimal::from(r.tokens.cache_read)
            * unit(&p.cache_read_cost_per_million))
        .normalize()
        .to_string());
        v["cacheCreationCostUsd"] = json!(((Decimal::from(
            r.tokens.cache_write - r.tokens.cache_write1h
        ) + Decimal::from(r.tokens.cache_write1h)
            * Decimal::new(16, 1))
            * unit(&p.cache_creation_cost_per_million))
        .normalize()
        .to_string());
    }
    v
}
pub fn query(store: &Store, kind: &str, args: &Value) -> Result<Value> {
    let scope = args["scope"].as_str().unwrap_or("local");
    let records = store.records(scope)?;
    let timezone: chrono_tz::Tz = store.config()?.timezone.parse()?;
    let start = args["startDate"].as_i64().unwrap_or(0);
    let end = args["endDate"].as_i64().unwrap_or(i64::MAX);
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
                    .is_none_or(|s| r.harness == s || provider(&r.harness) == s)
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
        "summary"=>Ok(summary(&selected)),
        "summaryByApp"=>{let mut groups:BTreeMap<String,Vec<UsageRecord>>=BTreeMap::new();for r in selected{groups.entry(r.harness.clone()).or_default().push(r)}Ok(json!(groups.into_iter().map(|(app,rs)|json!({"appType":app,"summary":summary(&rs)})).collect::<Vec<_>>()))},
        "firstDate"=>Ok(json!(selected.iter().filter_map(|r|timezone.timestamp_opt(r.occurred_at,0).single()).min().map(|d|d.format("%Y-%m-%d").to_string()))),
        "logs"=>{let mut rows:Vec<_>=selected.into_iter().filter(|r|r.grain=="event").collect();rows.sort_by(|a,b|b.occurred_at.cmp(&a.occurred_at).then(a.id.cmp(&b.id)));let total=rows.len();let page=args["page"].as_u64().unwrap_or(0) as usize;let size=args["pageSize"].as_u64().unwrap_or(20).clamp(1,200) as usize;Ok(json!({"data":rows.iter().skip(page.saturating_mul(size)).take(size).map(log).collect::<Vec<_>>(),"total":total,"page":page,"pageSize":size}))},
        "detail"=>Ok(selected.iter().find(|r|format!("{}|{}",r.device_id,r.id)==args["requestId"].as_str().unwrap_or("")&&args["deviceId"].as_str().is_none_or(|id|r.device_id==id)).map(log).unwrap_or(Value::Null)),
        "models"|"providers"=>{let mut groups:BTreeMap<String,Vec<UsageRecord>>=BTreeMap::new();for r in selected{groups.entry(if kind=="models"{r.model.clone()}else{r.harness.clone()}).or_default().push(r)}Ok(json!(groups.into_iter().map(|(key,rs)|{let s=summary(&rs);json!({"model":key,"providerId":key,"providerName":provider(&rs[0].harness),"appType":rs[0].harness,"requestCount":s["totalRequests"],"totalTokens":s["realTotalTokens"],"totalCost":s["totalCost"],"successRate":s["successRate"],"avgLatencyMs":0,"avgCostPerRequest":if s["totalRequests"].as_u64().unwrap_or(0)>0{(rs.iter().map(amount).sum::<Decimal>()/Decimal::from(s["totalRequests"].as_u64().unwrap())).normalize().to_string()}else{"0".into()}})}).collect::<Vec<_>>()))},
        "trends"=>{let hourly=end.saturating_sub(start)<=86_400;let mut groups:BTreeMap<String,Vec<UsageRecord>>=BTreeMap::new();for r in selected{let date=if let Some(day)=&r.legacy_date{day.clone()}else{timezone.timestamp_opt(r.occurred_at,0).single().map(|d|d.format(if hourly{"%Y-%m-%d %H:00:00"}else{"%Y-%m-%d"}).to_string()).unwrap_or_default()};groups.entry(date).or_default().push(r)}Ok(json!(groups.into_iter().map(|(date,rs)|{let s=summary(&rs);json!({"date":date,"requestCount":s["totalRequests"],"totalCost":s["totalCost"],"totalTokens":s["realTotalTokens"],"totalInputTokens":s["totalInputTokens"],"totalOutputTokens":s["totalOutputTokens"],"totalCacheCreationTokens":s["totalCacheCreationTokens"],"totalCacheReadTokens":s["totalCacheReadTokens"]})}).collect::<Vec<_>>()))},
        "sources"=>{let mut groups:BTreeMap<String,Vec<UsageRecord>>=BTreeMap::new();for r in selected{groups.entry(r.record_source.clone()).or_default().push(r)}Ok(json!(groups.into_iter().map(|(source,rs)|{let s=summary(&rs);json!({"dataSource":source,"requestCount":s["totalRequests"],"totalCostUsd":s["totalCost"]})}).collect::<Vec<_>>()))},
        "heatmap"=>Ok(json!(selected.iter().filter_map(|r|timezone.timestamp_opt(r.occurred_at,0).single().map(|d|json!({"date":d.format("%Y-%m-%d").to_string(),"weekday":d.weekday().num_days_from_monday(),"count":r.count}))).collect::<Vec<_>>())),
        _=>anyhow::bail!("Unknown usage query: {kind}")
    }
}
