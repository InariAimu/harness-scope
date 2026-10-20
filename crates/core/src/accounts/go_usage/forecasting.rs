//! Forward estimates from authoritative quota, using the sampled model's request mix.
use super::*;
use crate::pricing::go_catalog::{Model, Schedule};
use rust_decimal::prelude::ToPrimitive;

const FRESH_SECONDS: i64 = 600;
const MAX_HORIZON: i64 = 35 * 86400;

// Forecasts have sampled inputs; remove division dust from their display DTO.
fn decimal(value: Decimal) -> String {
    value.round_dp(18).normalize().to_string()
}

#[derive(Clone, Copy, Default)]
struct Rate {
    normalized: Decimal,
    usd: Decimal,
}
impl Rate {
    fn add(&mut self, other: Self) -> Result<()> {
        self.normalized = self
            .normalized
            .checked_add(other.normalized)
            .context("Forecast precision exceeded")?;
        self.usd = self
            .usd
            .checked_add(other.usd)
            .context("Forecast precision exceeded")?;
        Ok(())
    }
    fn scaled(self, scale: Decimal) -> Result<Self> {
        Ok(Self {
            normalized: self
                .normalized
                .checked_mul(scale)
                .context("Forecast precision exceeded")?,
            usd: self
                .usd
                .checked_mul(scale)
                .context("Forecast precision exceeded")?,
        })
    }
}
fn priced(
    model: &Model,
    catalog: &Catalog,
    fact: &Fact,
    peak: Option<bool>,
    plan: &str,
) -> Result<Rate> {
    anyhow::ensure!(
        fact.grain == "event",
        "Legacy usage cannot forecast a request mix"
    );
    let (usd, _, _) =
        go_catalog::quote(model, &catalog.peak_schedule, &fact.tokens, fact.time, peak)?;
    let limit = go_catalog::tier(model, &fact.tokens)
        .monthly_limits
        .for_plan(plan)?;
    let normalized = match limit {
        Some(limit) => usd
            .checked_div(limit)
            .context("Forecast precision exceeded")?,
        None => Decimal::ZERO,
    };
    Ok(Rate { normalized, usd })
}
struct Span {
    start: i64,
    end: i64,
    peak: bool,
}
fn spans(schedule: &Schedule, start: i64, end: i64) -> Result<Vec<Span>> {
    anyhow::ensure!(
        end >= start && end - start <= MAX_HORIZON,
        "Invalid forecast horizon"
    );
    let mut result = Vec::new();
    let mut at = start;
    while at < end {
        let day = Utc
            .timestamp_opt(at, 0)
            .single()
            .context("Invalid forecast time")?
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp();
        let next = schedule
            .intervals
            .iter()
            .flatten()
            .map(|minute| day + i64::from(*minute) * 60)
            .chain(std::iter::once(day + 86400))
            .filter(|value| *value > at)
            .min()
            .unwrap()
            .min(end);
        let peak = go_catalog::context(schedule, at).is_some_and(|v| v.period == "peak");
        result.push(Span {
            start: at,
            end: next,
            peak,
        });
        at = next;
    }
    Ok(result)
}
fn integrate(spans: &[Span], rates: [Rate; 2], remaining: Decimal) -> Result<(Rate, Option<i64>)> {
    let mut total = Rate::default();
    let mut left = remaining;
    let mut exhausted = if left <= Decimal::ZERO {
        spans.first().map(|v| v.start)
    } else {
        None
    };
    for span in spans {
        let rate = rates[usize::from(span.peak)];
        let cost = rate.scaled(Decimal::from(span.end - span.start))?;
        if exhausted.is_none() && rate.normalized > Decimal::ZERO && left <= cost.normalized {
            let seconds = left
                .checked_div(rate.normalized)
                .context("Forecast precision exceeded")?
                .round_dp(12)
                .ceil()
                .to_i64()
                .context("Forecast precision exceeded")?;
            exhausted = Some(span.start + seconds);
        }
        left = left
            .checked_sub(cost.normalized)
            .context("Forecast precision exceeded")?;
        total.add(cost)?;
    }
    Ok((total, exhausted))
}
fn unknown_window(id: &str, reason: &str, reset: Option<i64>) -> Value {
    json!({"id":id,"status":"unavailable","reason":reason,"resetsAt":reset})
}

pub(super) fn calculate(engine: &Engine, args: &Value) -> Result<Value> {
    calculate_at(engine, args, Utc::now().timestamp())
}
fn calculate_at(engine: &Engine, args: &Value, now: i64) -> Result<Value> {
    let id = id(args)?;
    let plan = plan(args)?;
    let minutes = args
        .get("lookbackMinutes")
        .map(|v| v.as_i64().context("Invalid sampling interval"))
        .transpose()?
        .unwrap_or(60);
    anyhow::ensure!(
        [15, 60, 1440].contains(&minutes),
        "Choose 15 minutes, 1 hour or 24 hours"
    );
    let pace = Decimal::from_str(
        args.get("pace")
            .map(|v| v.as_str().context("Invalid forecast pace"))
            .transpose()?
            .unwrap_or("1"),
    )?;
    anyhow::ensure!(
        pace >= Decimal::new(1, 1) && pace <= Decimal::from(10),
        "Forecast pace must be 0.1–10"
    );
    let store = engine.read_lock()?;
    store.transaction(|| {
        let item = load(&store)?.into_iter().find(|v| v.account.id == id).context("Provider account no longer exists")?;
        anyhow::ensure!(item.account.provider == "opencode-go", "Go forecast requires an OpenCode Go account");
        let catalog = go_catalog::load(&store)?;
        let seconds = minutes * 60;
        let records = facts(&store, &item.account, now - seconds, now)?;
        let latest = records.iter().filter(|f| f.grain == "event" && f.count > 0).max_by_key(|f| f.time);
        let requested = args.get("model").map(|v|v.as_str().context("Invalid forecast model")).transpose()?.filter(|s| !s.is_empty());
        let model = requested.or_else(|| latest.map(|f| f.model.as_str())).and_then(|raw| go_catalog::find(&catalog, raw));
        anyhow::ensure!(requested.is_none() || model.is_some(), "Go model not configured");
        let chosen: Vec<_> = records.iter().filter(|f| f.grain == "event" && model.is_some_and(|m| go_catalog::find(&catalog, &f.model).is_some_and(|v| v.id == m.id))).collect();
        let count: u64 = chosen.iter().map(|f| f.count).sum();
        let first = chosen.iter().map(|f| f.time).min();
        let last = chosen.iter().map(|f| f.time).max();
        let sample_span = first.zip(last).map_or(0, |(a,b)| b-a);
        let mut off = Rate::default(); let mut peak = Rate::default(); let mut observed = Rate::default();
        let mut tokens = 0u64; let mut unpriced = 0u64;
        if let Some(model) = model {
            for fact in &chosen {
                let prices = (|| -> Result<_> { Ok((priced(model,&catalog,fact,Some(false),plan)?,priced(model,&catalog,fact,Some(true),plan)?,priced(model,&catalog,fact,None,plan)?)) })();
                match prices {
                    Ok((a,b,c)) => {off.add(a)?;peak.add(b)?;observed.add(c)?;tokens=tokens.checked_add(fact.tokens.total()).context("Forecast token total exceeded")?;},
                    Err(_) => {unpriced+=fact.count;}
                }
            }
        }
        let sample_reason = if item.account.links.is_empty() {Some("no_links")}
            else if model.is_none() {Some("no_model")}
            else if model.is_some_and(|m| m.free) {Some("free_model")}
            else if unpriced > 0 {Some("unpriced_samples")}
            else if count < 3 || sample_span < 60 {Some("insufficient_samples")}
            else if off.normalized <= Decimal::ZERO && peak.normalized <= Decimal::ZERO {Some("zero_rate")}
            else {None};
        let scale = pace / Decimal::from(seconds);
        let rates = [off.scaled(scale)?,peak.scaled(scale)?];
        let current_peak = go_catalog::context(&catalog.peak_schedule,now).is_some_and(|v|v.period=="peak");
        let current = rates[usize::from(current_peak)];
        let requests_per_second = Decimal::from(count) * scale;
        let tokens_per_second = Decimal::from(tokens) * scale;
        let snapshot = item.snapshot.as_ref().filter(|s|s["data"]["kind"]=="quota");
        let fetched = snapshot.and_then(|s|s["fetchedAt"].as_i64());
        let snapshot_reason = match fetched {
            None => Some("no_quota"),
            Some(at) if at > now || now-at > FRESH_SECONDS => Some("stale_quota"),
            _ => None,
        };
        // Bring the official snapshot forward with linked usage from all synced devices.
        // Unknown post-snapshot usage blocks forecasts instead of overstating balance.
        let mut since = Decimal::ZERO; let mut since_unknown = 0u64;
        if snapshot_reason.is_none() {
            for fact in records.iter().filter(|f| f.time > fetched.unwrap()) {
                let value = go_catalog::find(&catalog,&fact.model).context("Unknown Go model")
                    .and_then(|m|priced(m,&catalog,fact,None,plan));
                match value {Ok(v)=>{since=since.checked_add(v.normalized).context("Forecast precision exceeded")?;},Err(_)=>{since_unknown+=fact.count;}}
            }
        }
        let mut windows = Vec::new();
        for (id, fraction) in [("rolling",&catalog.window_fractions.rolling),("weekly",&catalog.window_fractions.weekly),("monthly",&catalog.window_fractions.monthly)] {
            let row=snapshot.and_then(|s|s["data"]["windows"].as_array()).and_then(|v|v.iter().find(|r|r["id"]==id));
            let reset=row.and_then(|r|r["resetsAt"].as_i64());
            let reason=snapshot_reason.or_else(||if row.is_none(){Some("no_quota")}
                else if reset.is_none_or(|t|t<=now||t-now>MAX_HORIZON){Some("expired_window")}
                else if !row.is_some_and(|r|r["status"]=="ok"||r["status"]=="rate-limited"){Some("unknown_quota")}
                else if since_unknown>0 {Some("unpriced_since_snapshot")} else {sample_reason});
            if let Some(reason)=reason {windows.push(unknown_window(id,reason,reset));continue;}
            let reset=reset.unwrap();let row=row.unwrap();
            let used=Decimal::from_str(&row["usedPercent"].to_string())?;
            anyhow::ensure!(used>=Decimal::ZERO,"Invalid Go quota percentage");
            let fraction=Decimal::from_str(fraction)?;
            let remaining=if row["status"]=="rate-limited" {Decimal::ZERO} else {((Decimal::from(100)-used).max(Decimal::ZERO)/Decimal::from(100)*fraction-since).max(Decimal::ZERO)};
            let curve=spans(&catalog.peak_schedule,now,reset)?;
            let (expected,exhausted)=integrate(&curve,rates,remaining)?;
            if expected.normalized<=Decimal::ZERO {windows.push(unknown_window(id,"zero_rate",Some(reset)));continue;}
            let safe=remaining.checked_div(expected.normalized).context("Forecast precision exceeded")?;
            let duration=Decimal::from(reset-now);
            // Extremely tiny manually configured prices cannot yield usable budgets.
            if safe > Decimal::from(1_000_000_000u64) {windows.push(unknown_window(id,"precision_limit",Some(reset)));continue;}
            let request_budget=requests_per_second*duration*safe;
            let requests_day=requests_per_second*Decimal::from(86400)*safe;
            let tokens_day=tokens_per_second*Decimal::from(86400)*safe;
            let usd_day=expected.usd/duration*Decimal::from(86400)*safe;
            let event_at=exhausted.unwrap_or(reset).min(reset);
            windows.push(json!({"id":id,"status":"ready","reason":null,"resetsAt":reset,"remainingPercent":decimal(remaining/fraction*Decimal::from(100)),"postSnapshotPercent":decimal(since/fraction*Decimal::from(100)),"exhaustionAt":exhausted,"exhaustsBeforeReset":exhausted.is_some_and(|at|at<reset),"runtimeSeconds":event_at-now,"expectedUsdToReset":decimal(expected.usd),"remainingModelUsd":decimal(expected.usd*safe),"requestBudget":decimal(request_budget.round_dp(12).floor()),"requestsPerDay":decimal(requests_day),"requestsPerHour":decimal(requests_day/Decimal::from(24)),"tokensPerDay":decimal(tokens_day.floor()),"usdPerDay":decimal(usd_day),"activeHoursPerDay":decimal((safe*Decimal::from(24)).min(Decimal::from(24))),"safePaceMultiplier":decimal(safe)}));
        }
        let ready:Vec<_>=windows.iter().filter(|w|w["status"]=="ready").collect();
        // The forecast stops at the first reset as well as the first exhaustion.
        // It makes no promise about newly replenished rolling windows.
        let next=if ready.len()==3 {ready.iter().min_by_key(|w|now+w["runtimeSeconds"].as_i64().unwrap()).map(|w|json!({"window":w["id"],"at":now+w["runtimeSeconds"].as_i64().unwrap(),"kind":if w["exhaustsBeforeReset"]==true{"exhaustion"}else{"reset"}}))}else{None};
        let daily=if ready.len()==3 {ready.iter().min_by(|a,b|Decimal::from_str(a["requestsPerDay"].as_str().unwrap()).unwrap().cmp(&Decimal::from_str(b["requestsPerDay"].as_str().unwrap()).unwrap())).map(|w|json!({"window":w["id"],"requests":w["requestsPerDay"],"tokens":w["tokensPerDay"],"usd":w["usdPerDay"],"activeHours":w["activeHoursPerDay"]}))}else{None};
        Ok(json!({"asOf":now,"catalogRevision":go_catalog::revision(&catalog),"model":model.map(|m|m.id.as_str()),"modelName":model.map(|m|m.name.as_str()),"latestRawModel":latest.map(|f|f.model.as_str()),"plan":plan,"lookbackMinutes":minutes,"pace":decimal(pace),"quotaFetchedAt":fetched,"quotaMaxAgeSeconds":FRESH_SECONDS,"sample":{"requests":count,"spanSeconds":sample_span,"unpricedRequests":unpriced,"reason":sample_reason,"observedUsdPerHour":decimal(observed.usd*Decimal::from(3600)/Decimal::from(seconds)),"currentUsdPerHour":decimal(current.usd*Decimal::from(3600)),"requestsPerHour":decimal(requests_per_second*Decimal::from(3600)),"monthlyPercentPerHour":decimal(current.normalized*Decimal::from(360000)),"firstAt":first,"lastAt":last},"windows":windows,"nextEvent":next,"dailyBudget":daily}))
    })
}

#[cfg(test)]
mod tests;
