use super::*;
use crate::{
    model::Tokens,
    pricing::go_catalog::{self, Catalog},
};
use rusqlite::params_from_iter;
mod forecasting;

pub(super) fn forecast(engine: &Engine, args: &Value) -> Result<Value> {
    forecasting::calculate(engine, args)
}

fn plan(args: &Value) -> Result<&str> {
    let value = args["plan"].as_str().context("Choose an estimation plan")?;
    anyhow::ensure!(
        ["go", "goPlus"].contains(&value),
        "Choose Go or Go Plus for allowance estimates"
    );
    Ok(value)
}
fn decimal(value: Decimal) -> String {
    value.normalize().to_string()
}
#[derive(Default)]
struct Total {
    requests: u64,
    unmatched: u64,
    cost: Decimal,
    percent: Decimal,
}
impl Total {
    fn value(&self) -> Value {
        json!({"requests":self.requests,"unmatchedRequests":self.unmatched,"costUsd":decimal(self.cost),"percent":decimal(self.percent)})
    }
}
struct Fact {
    model: String,
    time: i64,
    count: u64,
    grain: String,
    tokens: Tokens,
}
fn facts(store: &Store, account: &Account, start: i64, end: i64) -> Result<Vec<Fact>> {
    let mut selection = Selection::new(store, &json!({"scope":"local"}), start, end)?;
    if account.links.is_empty() {
        return Ok(vec![]);
    }
    selection.sql.push_str(" AND (");
    for (i, link) in account.links.iter().enumerate() {
        if i > 0 {
            selection.sql.push_str(" OR ");
        }
        selection.sql.push_str("(u.harness=? AND u.provider=?)");
        selection
            .params
            .extend([link.harness.clone().into(), link.provider.clone().into()]);
    }
    selection.sql.push(')');
    // Decode only the optional 1h-write subset, and only for rows with cache writes.
    let mut stmt=store.conn.prepare_cached(&format!("SELECT u.model,u.occurred_at,u.request_count,u.grain,u.input_tokens,u.output_tokens,u.cache_read,u.cache_write,CASE WHEN u.cache_write>0 THEN COALESCE(json_extract(r.payload,'$.tokens.cacheWrite1h'),0) ELSE 0 END FROM usage_index u LEFT JOIN records r ON r.device_id=u.device_id AND r.id=u.id WHERE {}",selection.sql))?;
    let rows = stmt
        .query_map(params_from_iter(selection.params.iter()), |r| {
            Ok(Fact {
                model: r.get(0)?,
                time: r.get(1)?,
                count: r.get(2)?,
                grain: r.get(3)?,
                tokens: Tokens {
                    input: r.get(4)?,
                    output: r.get(5)?,
                    cache_read: r.get(6)?,
                    cache_write: r.get(7)?,
                    cache_write1h: r.get(8)?,
                    ..Default::default()
                },
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}
struct Window {
    id: &'static str,
    start: i64,
    end: i64,
    basis: &'static str,
    fraction: Decimal,
}
fn windows(
    account: &Account,
    snapshot: Option<&Value>,
    catalog: &Catalog,
    now: i64,
    timezone: chrono_tz::Tz,
) -> Result<Vec<Window>> {
    use chrono::{Months, TimeZone};
    let time = Utc
        .timestamp_opt(now, 0)
        .single()
        .context("Invalid estimation time")?;
    let manual = account
        .subscription
        .period_start
        .as_deref()
        .map(|v| midnight(timezone, NaiveDate::parse_from_str(v, "%Y-%m-%d")?))
        .transpose()?;
    let mut result = Vec::new();
    for (id, seconds, fraction) in [
        ("rolling", 18000, &catalog.window_fractions.rolling),
        ("weekly", 604800, &catalog.window_fractions.weekly),
        ("monthly", 0, &catalog.window_fractions.monthly),
    ] {
        let reset = snapshot
            .filter(|s| {
                s["data"]["kind"] == "quota" && s["fetchedAt"].as_i64().is_some_and(|v| v <= now)
            })
            .and_then(|s| s["data"]["windows"].as_array())
            .and_then(|v| v.iter().find(|v| v["id"] == id))
            .and_then(|v| v["resetsAt"].as_i64())
            .filter(|v| *v > now);
        let (start, basis) = if let Some(reset) = reset {
            let start = if id == "monthly" {
                Utc.timestamp_opt(reset, 0)
                    .single()
                    .context("Invalid Go reset")?
                    .checked_sub_months(Months::new(1))
                    .context("Invalid monthly Go window")?
                    .timestamp()
            } else {
                reset - seconds
            };
            (start, "provider_window")
        } else if id == "monthly" {
            (
                manual.unwrap_or(
                    time.checked_sub_months(Months::new(1))
                        .context("Invalid month")?
                        .timestamp(),
                ),
                if manual.is_some() {
                    "manual_period"
                } else {
                    "lookback"
                },
            )
        } else {
            (now - seconds, "lookback")
        };
        let end = if basis == "manual_period" {
            account
                .subscription
                .renewal_date
                .as_deref()
                .map(|v| {
                    Ok::<_, anyhow::Error>(
                        midnight(timezone, NaiveDate::parse_from_str(v, "%Y-%m-%d")?)? - 1,
                    )
                })
                .transpose()?
                .unwrap_or(now)
                .min(now)
        } else {
            now
        };
        result.push(Window {
            id,
            start,
            end,
            basis,
            fraction: Decimal::from_str(fraction)?,
        });
    }
    Ok(result)
}
pub(super) fn estimate(engine: &Engine, args: &Value) -> Result<Value> {
    let id = id(args)?;
    let plan = plan(args)?;
    let store = engine.read_lock()?;
    store.transaction(||{
        let item=load(&store)?.into_iter().find(|v|v.account.id==id).context("Provider account no longer exists")?;
        anyhow::ensure!(item.account.provider=="opencode-go","Go estimates require an OpenCode Go account");
        let catalog=go_catalog::load(&store)?;let timezone=store.config()?.timezone.parse()?;let now=Utc::now().timestamp();
        let windows=windows(&item.account,item.snapshot.as_ref(),&catalog,now,timezone)?;
        let start=windows.iter().map(|w|w.start).min().unwrap().min(now-3600);
        let facts=facts(&store,&item.account,start,now)?;
        let mut results=Vec::new();
        for window in &windows {
            let mut total=Total::default();let mut models:BTreeMap<String,(Total,Option<String>)>=BTreeMap::new();
            for fact in facts.iter().filter(|f|f.time>=window.start&&f.time<=window.end){
                let group=models.entry(fact.model.clone()).or_default();total.requests+=fact.count;group.0.requests+=fact.count;
                let calculated=(||->Result<(Decimal,Decimal,String)>{
                    anyhow::ensure!(fact.grain=="event","Legacy daily usage cannot be estimated precisely");
                    let model=go_catalog::find(&catalog,&fact.model).context("Go model not configured")?;
                    let (cost,_,_)=go_catalog::quote(model,&catalog.peak_schedule,&fact.tokens,fact.time,None)?;
                    let limit=go_catalog::tier(model,&fact.tokens).monthly_limits.for_plan(plan)?;
                    let percent=go_catalog::allowance_percent(cost,limit,window.fraction)?;
                    Ok((cost,percent,model.id.clone()))
                })();
                match calculated{Ok((cost,percent,model))=>{
                    let next=total.cost.checked_add(cost).zip(total.percent.checked_add(percent)).zip(group.0.cost.checked_add(cost).zip(group.0.percent.checked_add(percent)));
                    if let Some(((a,b),(c,d)))=next{total.cost=a;total.percent=b;group.0.cost=c;group.0.percent=d;group.1=Some(model);}
                    else{total.unmatched+=fact.count;group.0.unmatched+=fact.count;}
                },Err(_)=>{total.unmatched+=fact.count;group.0.unmatched+=fact.count;}}
            }
            let mut models=models.into_iter().map(|(model,(total,pricing))|json!({"model":model,"pricingModel":pricing,"usage":total.value()})).collect::<Vec<_>>();
            models.sort_by(|a,b|Decimal::from_str(b["usage"]["percent"].as_str().unwrap()).unwrap().cmp(&Decimal::from_str(a["usage"]["percent"].as_str().unwrap()).unwrap()).then_with(||a["model"].as_str().cmp(&b["model"].as_str())));
            results.push(json!({"id":window.id,"start":window.start,"end":window.end,"basis":window.basis,"total":total.value(),"models":models}));
        }
        let mut recent:BTreeMap<String,(u64,Tokens)>=BTreeMap::new();
        for fact in facts.iter().filter(|f|f.time>=now-3600&&f.grain=="event") {
            if let Some(model)=go_catalog::find(&catalog,&fact.model){let (count,t)=recent.entry(model.id.clone()).or_default();*count+=fact.count;t.input+=fact.tokens.input;t.output+=fact.tokens.output;t.cache_read+=fact.tokens.cache_read;t.cache_write+=fact.tokens.cache_write;t.cache_write1h+=fact.tokens.cache_write1h;}
        }
        let recent=recent.into_iter().filter(|(_,v)|v.0>0).map(|(model,(count,t))|json!({"model":model,"requests":count,"averageTokens":{"input":t.input/count,"output":t.output/count,"cacheRead":t.cache_read/count,"cacheWrite":t.cache_write/count,"cacheWrite1h":t.cache_write1h/count,"reasoning":0}})).collect::<Vec<_>>();
        Ok(json!({"plan":plan,"catalogRevision":go_catalog::revision(&catalog),"windows":results,"recentHour":recent,"asOf":now,"linked":!item.account.links.is_empty()}))
    })
}
pub(super) fn simulate(engine: &Engine, args: &Value) -> Result<Value> {
    let plan = plan(args)?;
    let catalog = go_catalog::load(&*engine.read_lock()?)?;
    let model = go_catalog::find(
        &catalog,
        args["model"].as_str().context("Choose a Go model")?,
    )
    .context("Go model not configured")?;
    let tokens: Tokens = serde_json::from_value(args["tokens"].clone())?;
    let requests = args["requests"]
        .as_u64()
        .filter(|v| *v <= 1_000_000_000)
        .context("Invalid simulated request count")?;
    let time = args["timestamp"]
        .as_i64()
        .unwrap_or_else(|| Utc::now().timestamp());
    let forced = match args["period"].as_str().unwrap_or("current") {
        "current" => None,
        "peak" => Some(true),
        "off_peak" => Some(false),
        _ => anyhow::bail!("Invalid Go simulation period"),
    };
    let (unit, context, _) =
        go_catalog::quote(model, &catalog.peak_schedule, &tokens, time, forced)?;
    let cost = unit * Decimal::from(requests);
    let limit = go_catalog::tier(model, &tokens)
        .monthly_limits
        .for_plan(plan)?;
    let base = go_catalog::allowance_percent(cost, limit, Decimal::ONE)?;
    let rolling = go_catalog::allowance_percent(
        cost,
        limit,
        Decimal::from_str(&catalog.window_fractions.rolling)?,
    )?;
    let weekly = go_catalog::allowance_percent(
        cost,
        limit,
        Decimal::from_str(&catalog.window_fractions.weekly)?,
    )?;
    Ok(
        json!({"model":model.id,"plan":plan,"period":context.map(|c|c.period),"requests":requests,"perRequestUsd":decimal(unit),"costUsd":decimal(cost),"monthlyLimit":limit.map(decimal),"percent":{"rolling":decimal(rolling),"weekly":decimal(weekly),"monthly":decimal(base)},"catalogRevision":go_catalog::revision(&catalog)}),
    )
}
