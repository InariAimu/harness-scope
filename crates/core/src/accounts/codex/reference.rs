use super::*;
use crate::{pricing, query_store::Row};
use rust_decimal::prelude::ToPrimitive;

fn decimal(value: Decimal) -> String {
    value.round_dp(18).normalize().to_string()
}
fn mul(a: Decimal, b: Decimal) -> Result<Decimal> {
    a.checked_mul(b)
        .context("Reference estimate precision exceeded")
}
fn div(a: Decimal, b: Decimal) -> Result<Decimal> {
    a.checked_div(b)
        .context("Reference estimate precision exceeded")
}
pub(super) fn calculate(engine: &Engine, args: &Value) -> Result<Value> {
    calculate_at(engine, args, Utc::now().timestamp())
}
fn calculate_at(engine: &Engine, args: &Value, now: i64) -> Result<Value> {
    let id = id(args)?;
    let minutes = args
        .get("lookbackMinutes")
        .map(|v| v.as_i64().context("Invalid sampling interval"))
        .transpose()?
        .unwrap_or(0);
    anyhow::ensure!(
        [0, 15, 60, 1440].contains(&minutes),
        "Choose the current window, 15 minutes, 1 hour or 24 hours"
    );
    let pace = Decimal::from_str(
        args.get("pace")
            .map(|v| v.as_str().context("Invalid usage intensity"))
            .transpose()?
            .unwrap_or("1"),
    )?;
    anyhow::ensure!(
        pace >= Decimal::new(1, 1) && pace <= Decimal::from(10),
        "Usage intensity must be 0.1–10"
    );
    let requested = args
        .get("model")
        .map(|v| v.as_str().context("Invalid forecast model"))
        .transpose()?
        .filter(|v| !v.is_empty());
    let bucket_id = args
        .get("bucket")
        .map(|v| v.as_str().context("Invalid quota bucket"))
        .transpose()?
        .unwrap_or("codex");
    let store = engine.read_lock()?;
    store.transaction(|| {
        let item=load(&store)?.into_iter().find(|v|v.account.id==id).context("Provider account no longer exists")?;
        anyhow::ensure!(item.account.provider=="openai-codex","Reference estimate requires a Codex subscription account");
        let settings=item.account.codex.as_ref().context("Choose a Codex estimation plan")?;
        let plan=args.get("plan").map(|v|v.as_str().context("Invalid reference plan")).transpose()?.unwrap_or(&settings.plan);
        let catalog=catalog::load(&store)?;
        let plan_info=catalog::plan(&catalog,plan)?;
        let mode=args.get("mode").map(|v|v.as_str().context("Invalid Codex allowance mode")).transpose()?.unwrap_or(&catalog.default_mode);
        anyhow::ensure!(["modeldial","theoretical"].contains(&mode),"Invalid Codex allowance mode");
        let factor=plan_info.theoretical_multiplier;
        let snapshot=item.snapshot.as_ref().filter(|s|s["data"]["kind"]=="codexQuota");
        let fetched=snapshot.and_then(|s|s["fetchedAt"].as_i64());
        let bucket=snapshot.and_then(|s|s["data"]["buckets"].as_array()).and_then(|v|v.iter().find(|v|v["id"]==bucket_id));
        let official_windows=bucket.and_then(|b|b["windows"].as_array());
        // The weekly period is the shared baseline; fall back to an actual 5h window.
        let anchor=official_windows.and_then(|v|v.iter().find(|w|w["durationMinutes"]==10080).or_else(||v.iter().find(|w|w["durationMinutes"]==300)));
        let window_minutes=anchor.and_then(|w|w["durationMinutes"].as_i64());
        let window_start=anchor.and_then(|w|{
            let duration=w["durationMinutes"].as_i64()?.checked_mul(60)?;
            let reset=w["resetsAt"].as_i64()?;
            let start=reset.checked_sub(duration)?;
            (reset>now && start<=now).then_some(start)
        });
        let sample_start=window_start.map(|start|if minutes==0 {start}else{start.max(now-minutes*60)}).unwrap_or(now);
        let sample_seconds=(now-sample_start).max(1);
        let selection=linked_selection(&store,&item.account,sample_start,now)?;
        let records=selection.rows_with_metadata(&store)?;
        let events:Vec<&Row>=records.iter().filter(|r|r.grain=="event" && r.count>0).collect();
        let model=requested.or_else(||events.iter().max_by_key(|r|r.occurred_at).map(|r|r.model.as_str()));
        let allowance_model=model.and_then(|m|catalog::find(&catalog,m));
        let models:std::collections::BTreeSet<_>=events.iter().map(|r|r.model.clone()).collect();
        let selected:Vec<_>=events.iter().filter(|r|if let Some(m)=allowance_model {catalog::find(&catalog,&r.model).is_some_and(|v|v.id==m.id)}else{Some(r.model.as_str())==model}).collect();
        let first=selected.iter().map(|r|r.occurred_at).min();let last=selected.iter().map(|r|r.occurred_at).max();
        let span=first.zip(last).map_or(0,|(a,b)|b-a);
        let prices=store.prices()?;let pricing_settings=store.pricing_settings()?;
        let quote=|row:&Row| {
            pricing::find_for_provider(&prices,&row.model,Some("openai")).or_else(||pricing::find(&prices,&row.model))
                .or_else(||catalog::find(&catalog,&row.model).and_then(|m|pricing::find_for_provider(&prices,&m.id,Some("openai")).or_else(||pricing::find(&prices,&m.id))))
                .and_then(|p|pricing::estimate_at(&row.tokens,p,now,&pricing_settings).ok()).and_then(|c|c.amount_usd).and_then(|s|Decimal::from_str(&s).ok())
        };
        let mut cost=Decimal::ZERO;let mut unpriced=0u64;let mut count=0u64;let mut tokens=0u64;
        for row in &selected {
            count=count.checked_add(row.count).context("Too many sample requests")?;
            tokens=tokens.checked_add(row.tokens.total()).context("Too many sample tokens")?;
            let quoted=quote(row);
            if let Some(value)=quoted {cost=cost.checked_add(value).context("Reference estimate precision exceeded")?;} else {unpriced=unpriced.saturating_add(row.count);}
        }
        let reason=if item.account.links.is_empty(){Some("no_links")}
            else if window_start.is_none(){Some("no_quota")}
            else if model.is_none(){Some("no_model")}
            else if unpriced>0 {Some("unpriced_samples")}
            else if count<3 || span<60 {Some("insufficient_samples")}
            else if cost<=Decimal::ZERO {Some("zero_rate")} else {None};
        let rate=mul(div(cost,Decimal::from(sample_seconds))?,pace)?;
        let request_rate=mul(div(Decimal::from(count),Decimal::from(sample_seconds))?,pace)?;
        let token_rate=mul(div(Decimal::from(tokens),Decimal::from(sample_seconds))?,pace)?;
        let snapshot_reason=if snapshot.is_none(){Some("no_quota")} else if fetched.is_none_or(|t|t>now || now-t>600){Some("stale_quota")} else if bucket.is_none(){Some("no_quota")} else {None};
        let mut windows=Vec::new();
        let window_types:Vec<(&str,i64)>=if plan=="plus"{vec![("rolling",300),("weekly",10080)]}else{vec![("weekly",10080)]};
        for (id,window_minutes) in window_types {
            let capacity=allowance_model.map(|m|catalog::resolve(&catalog,m,plan,id,mode)).transpose()?.flatten();
            let reference_budget=capacity.as_ref().map(|v|Decimal::from_str(&v.api_equivalent_usd)).transpose()?;
            let official=bucket.and_then(|b|b["windows"].as_array()).and_then(|v|v.iter().find(|w|w["durationMinutes"]==window_minutes));
            let reset=official.and_then(|w|w["resetsAt"].as_i64());
            let unavailable=if reference_budget.is_none(){Some("no_model_budget")}else if !catalog.bucket_ids.iter().any(|b|b==bucket_id){Some("unsupported_bucket")}else{snapshot_reason.or_else(||if official.is_none(){Some("no_quota")}
                else if reset.is_none_or(|r|r<=now || r-now>35*86400){Some("expired_window")}
                else if !official.is_some_and(|w|w["status"]=="ok"||w["status"]=="rate-limited"){Some("unknown_quota")} else {reason})};
            let mut result=json!({"id":id,"durationMinutes":window_minutes,"budgetUsd":reference_budget.map(decimal),"capacity":capacity,"resetsAt":reset,"status":"unavailable","reason":unavailable});
            if let Some(row)=official {
                let percent=Decimal::from_str(&row["usedPercent"].to_string())?;
                let percent=if row["status"]=="rate-limited"{Decimal::ZERO}else{(Decimal::from(100)-percent).clamp(Decimal::ZERO,Decimal::from(100))};
                result["officialRemainingPercent"]=json!(decimal(percent));
                if let Some(budget)=reference_budget {result["remainingUsd"]=json!(decimal(mul(budget,percent)?/Decimal::from(100)));}
            }
            if unavailable.is_none() {
                let reset=reset.unwrap();let seconds=Decimal::from(reset-now);
                let remaining=Decimal::from_str(result["remainingUsd"].as_str().unwrap())?;
                let lifetime=div(remaining,rate)?;
                let runtime=lifetime.min(seconds).round_dp(12).ceil().to_i64().context("Reference estimate precision exceeded")?;
                let safe=div(remaining,mul(rate,seconds)?)?;
                anyhow::ensure!(safe<=Decimal::from(1_000_000_000u64),"Reference estimate precision exceeded");
                let requests=mul(mul(request_rate,seconds)?,safe)?;
                let requests_daily=mul(div(requests,seconds)?,Decimal::from(86400))?;
                result=json!({"id":id,"durationMinutes":window_minutes,"budgetUsd":reference_budget.map(decimal),"capacity":capacity,"resetsAt":reset,"status":"ready","reason":null,
                    "officialRemainingPercent":result["officialRemainingPercent"],"remainingUsd":decimal(remaining),"runtimeSeconds":runtime,"exhaustsBeforeReset":lifetime<=seconds,
                    "requestBudget":decimal(requests.round_dp(12).floor()),"requestsPerDay":decimal(requests_daily),"requestsPerHour":decimal(requests_daily/Decimal::from(24)),
                    "tokensPerDay":decimal(mul(mul(token_rate,safe)?,Decimal::from(86400))?),"usdPerDay":decimal(div(remaining,seconds)?*Decimal::from(86400)),"activeHoursPerDay":decimal((safe*Decimal::from(24)).min(Decimal::from(24))),"requiredIntensity":decimal(safe*pace)});
            }
            windows.push(result);
        }
        let ready:Vec<_>=windows.iter().filter(|w|w["status"]=="ready").collect();
        let complete=ready.len()==windows.len();
        let next=if complete{ready.iter().min_by_key(|w|w["runtimeSeconds"].as_i64().unwrap()).map(|w|json!({"window":w["id"],"at":now+w["runtimeSeconds"].as_i64().unwrap(),"kind":if w["exhaustsBeforeReset"]==true{"exhaustion"}else{"reset"}}))}else{None};
        let daily=if complete{ready.iter().min_by(|a,b|Decimal::from_str(a["usdPerDay"].as_str().unwrap()).unwrap().cmp(&Decimal::from_str(b["usdPerDay"].as_str().unwrap()).unwrap())).map(|w|json!({"window":w["id"],"requests":w["requestsPerDay"],"tokens":w["tokensPerDay"],"usd":w["usdPerDay"]}))}else{None};
        let calibrated=calibration::calculate(&store,calibration::Input{id,snapshot,bucket:bucket_id,catalog:&catalog,plan,mode,model:allowance_model,now,lookback:sample_seconds,rate,request_rate,pace,sample_valid:reason.is_none()},&events,quote)?;
        Ok(json!({"asOf":now,"model":model,"models":models,"allowanceModel":allowance_model.map(|m|m.id.as_str()),"plan":plan,"multiplier":factor,"monthlyFeeUsd":plan_info.monthly_fee_usd,"mode":mode,"catalogRevision":catalog::revision(&catalog),"catalogVersion":catalog.version,"lookbackMinutes":minutes,"pace":decimal(pace),"quotaFetchedAt":fetched,"bucket":bucket_id,"calibration":calibrated,
            "sample":{"startDate":sample_start,"endDate":now,"durationSeconds":now-sample_start,"windowStart":window_start,"windowMinutes":window_minutes,"requests":count,"tokens":tokens,"unpricedRequests":unpriced,"costUsd":decimal(cost),"spanSeconds":span,"lastAt":last,"reason":reason,"rateUsdPerHour":decimal(rate*Decimal::from(3600)),"requestsPerHour":decimal(request_rate*Decimal::from(3600))},"windows":windows,"nextEvent":next,"dailyBudget":daily}))
    })
}

#[cfg(test)]
mod tests;
