//! Local, versioned Go allowance rules. Files are drafts until validated and applied.
use crate::{
    model::{Price, PricingContext, Tokens},
    store::Store,
};
use anyhow::{Context, Result};
use chrono::{Datelike, Timelike};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashSet, fs, path::Path, str::FromStr};

const META: &str = "opencode_go_catalog_v1";
pub const RULE: &str = "opencode-go-catalog-v1";
const MAX_BYTES: usize = 262144;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Rates {
    pub input: String,
    pub output: String,
    pub cache_read: String,
    pub cache_write: Option<String>,
    pub cache_write1h: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Limits {
    pub go: Option<String>,
    pub go_plus: Option<String>,
}
impl Limits {
    pub fn for_plan(&self, plan: &str) -> Result<Option<Decimal>> {
        let value = match plan {
            "go" => &self.go,
            "goPlus" => &self.go_plus,
            _ => anyhow::bail!("Choose Go or Go Plus for allowance estimates"),
        };
        value
            .as_deref()
            .map(Decimal::from_str)
            .transpose()
            .map_err(Into::into)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Tier {
    pub max_context_tokens: Option<u64>,
    pub prices: Rates,
    pub peak_prices: Option<Rates>,
    pub monthly_limits: Limits,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub aliases: Vec<String>,
    pub free: bool,
    pub notes: String,
    pub sample_tokens: Option<Tokens>,
    pub tiers: Vec<Tier>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Schedule {
    pub timezone: String,
    pub weekdays: Vec<u32>,
    pub intervals: Vec<[u32; 2]>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Fractions {
    pub rolling: String,
    pub weekly: String,
    pub monthly: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: u32,
    pub version: String,
    pub source_url: String,
    pub checked_at: String,
    pub notes: String,
    pub window_fractions: Fractions,
    pub peak_schedule: Schedule,
    pub models: Vec<Model>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Tariff {
    pub catalog_version: String,
    pub revision: String,
    pub model: Model,
    pub peak_schedule: Schedule,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_rates: Option<Rates>,
}
pub fn defaults() -> Catalog {
    serde_json::from_str(include_str!("../../resources/opencode-go-catalog.json"))
        .expect("vendored Go catalog")
}
pub fn revision(catalog: &Catalog) -> String {
    crate::harness::files::hash(&serde_json::to_vec(catalog).expect("serializable catalog"))
}
pub fn load(store: &Store) -> Result<Catalog> {
    let catalog = store.meta(META)?.unwrap_or_else(defaults);
    validate(&catalog)?;
    Ok(catalog)
}
fn number(value: &str, positive: bool) -> Result<Decimal> {
    anyhow::ensure!(value.len() <= 40, "Go decimal is too long");
    let value = Decimal::from_str(value).context("Invalid Go decimal")?;
    anyhow::ensure!(
        value
            >= if positive {
                Decimal::new(1, 6)
            } else {
                Decimal::ZERO
            }
            && value <= Decimal::from(1_000_000),
        "Go decimal out of range"
    );
    Ok(value)
}
fn validate_rates(rates: &Rates, free: bool) -> Result<()> {
    for value in [
        Some(&rates.input),
        Some(&rates.output),
        Some(&rates.cache_read),
        rates.cache_write.as_ref(),
        rates.cache_write1h.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        let n = number(value, false)?;
        anyhow::ensure!(
            !free || n == Decimal::ZERO,
            "Free Go model has nonzero prices"
        );
    }
    Ok(())
}
fn validate_model(model: &Model) -> Result<()> {
    anyhow::ensure!(
        !model.id.is_empty()
            && model.id.len() <= 128
            && model
                .id
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b".-_".contains(&c)),
        "Invalid Go model ID"
    );
    anyhow::ensure!(
        !model.name.is_empty()
            && model.name.chars().count() <= 100
            && model.notes.len() <= 1000
            && model.aliases.len() <= 32,
        "Invalid Go model metadata"
    );
    anyhow::ensure!(
        !model.tiers.is_empty() && model.tiers.len() <= 8,
        "Go model needs 1–8 context tiers"
    );
    let mut last = 0;
    for (i, tier) in model.tiers.iter().enumerate() {
        match tier.max_context_tokens {
            Some(value) => {
                anyhow::ensure!(
                    i + 1 < model.tiers.len() && value > last && value <= 1_000_000_000,
                    "Go context tiers must increase and end with null"
                );
                last = value;
            }
            None => anyhow::ensure!(
                i + 1 == model.tiers.len(),
                "Only the last Go context tier may end with null"
            ),
        }
        validate_rates(&tier.prices, model.free)?;
        if let Some(peak) = &tier.peak_prices {
            validate_rates(peak, model.free)?;
        }
        for limit in [&tier.monthly_limits.go, &tier.monthly_limits.go_plus] {
            if model.free {
                anyhow::ensure!(
                    limit.is_none(),
                    "Free model allowance must be unlimited (null)"
                );
            } else {
                number(
                    limit
                        .as_deref()
                        .context("Paid Go model needs both plan allowances")?,
                    true,
                )?;
            }
        }
    }
    if let Some(tokens) = &model.sample_tokens {
        validate_tokens(tokens)?;
    }
    Ok(())
}
fn validate_schedule(schedule: &Schedule) -> Result<()> {
    anyhow::ensure!(
        schedule.timezone == "UTC"
            && !schedule.weekdays.is_empty()
            && schedule.weekdays.len() <= 7
            && schedule.intervals.len() <= 12,
        "Go peak schedule must use UTC weekdays"
    );
    let mut days = HashSet::new();
    for day in &schedule.weekdays {
        anyhow::ensure!(
            (1..=7).contains(day) && days.insert(day),
            "Invalid Go peak weekday"
        );
    }
    let mut end = 0;
    for [a, b] in &schedule.intervals {
        anyhow::ensure!(
            *a >= end && a < b && *b <= 1440,
            "Go peak intervals must be ordered and non-overlapping"
        );
        end = *b;
    }
    Ok(())
}
pub fn validate(catalog: &Catalog) -> Result<()> {
    anyhow::ensure!(
        catalog.schema_version == 1
            && catalog.version.len() <= 64
            && !catalog.version.is_empty()
            && catalog
                .version
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b".-_".contains(&c)),
        "Invalid Go catalog version"
    );
    anyhow::ensure!(
        catalog.source_url == "https://opencode.ai/v2/docs/console/go/#usage-limits"
            && catalog.notes.len() <= 2000,
        "Go catalog must cite the official usage table"
    );
    let day = chrono::NaiveDate::parse_from_str(&catalog.checked_at, "%Y-%m-%d")?;
    anyhow::ensure!(
        day.format("%Y-%m-%d").to_string() == catalog.checked_at,
        "Invalid Go checked date"
    );
    let r = number(&catalog.window_fractions.rolling, true)?;
    let w = number(&catalog.window_fractions.weekly, true)?;
    let m = number(&catalog.window_fractions.monthly, true)?;
    anyhow::ensure!(
        r <= w && w <= m && m == Decimal::ONE,
        "Invalid Go window fractions"
    );
    validate_schedule(&catalog.peak_schedule)?;
    anyhow::ensure!(
        !catalog.models.is_empty() && catalog.models.len() <= 200,
        "Go catalog needs 1–200 models"
    );
    let mut ids = HashSet::new();
    for model in &catalog.models {
        validate_model(model)?;
        for id in std::iter::once(&model.id).chain(model.aliases.iter()) {
            anyhow::ensure!(
                !id.is_empty()
                    && id.len() <= 128
                    && id.bytes().all(|c| c.is_ascii_lowercase()
                        || c.is_ascii_digit()
                        || b".-_".contains(&c))
                    && ids.insert(id),
                "Duplicate or invalid Go model ID / alias"
            );
        }
    }
    anyhow::ensure!(
        serde_json::to_vec(catalog)?.len() <= MAX_BYTES,
        "Go catalog is too large"
    );
    Ok(())
}
pub fn validate_tariff(tariff: &Tariff) -> Result<()> {
    anyhow::ensure!(
        tariff.catalog_version.len() <= 64
            && tariff.revision.len() == 64
            && tariff.revision.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid Go tariff revision"
    );
    validate_schedule(&tariff.peak_schedule)?;
    validate_model(&tariff.model)?;
    if let Some(rates) = &tariff.selected_rates {
        validate_rates(rates, tariff.model.free)?;
    }
    Ok(())
}
pub fn validate_tokens(tokens: &Tokens) -> Result<()> {
    anyhow::ensure!(
        tokens.total() <= 1_000_000_000_000
            && tokens.cache_write1h <= tokens.cache_write
            && tokens.reasoning <= tokens.output,
        "Invalid Go simulation token counts"
    );
    Ok(())
}
pub fn find<'a>(catalog: &'a Catalog, raw: &str) -> Option<&'a Model> {
    if let Some(canonical) = super::deepseek::canonical(raw) {
        if let Some(m) = catalog
            .models
            .iter()
            .find(|m| m.id == canonical || m.aliases.iter().any(|a| a == canonical))
        {
            return Some(m);
        }
    }
    let candidates = crate::pricing_match::model_pricing_candidates(raw);
    catalog.models.iter().find(|model| {
        candidates.iter().any(|c| {
            model.id.eq_ignore_ascii_case(c)
                || model.aliases.iter().any(|a| a.eq_ignore_ascii_case(c))
        })
    })
}
pub fn context(schedule: &Schedule, timestamp: i64) -> Option<PricingContext> {
    use chrono::TimeZone;
    let time = chrono::Utc.timestamp_opt(timestamp, 0).single()?;
    if timestamp <= 0 {
        return None;
    }
    let minute = time.hour() * 60 + time.minute();
    let peak = schedule
        .weekdays
        .contains(&time.weekday().number_from_monday())
        && schedule
            .intervals
            .iter()
            .any(|[a, b]| (*a..*b).contains(&minute));
    Some(PricingContext {
        period: if peak { "peak" } else { "off_peak" }.into(),
        timestamp,
        time_basis: "log_timestamp".into(),
        calendar_version: "UTC-local-Go-catalog".into(),
    })
}
pub fn tier<'a>(model: &'a Model, tokens: &Tokens) -> &'a Tier {
    let context = tokens
        .input
        .saturating_add(tokens.cache_read)
        .saturating_add(tokens.cache_write);
    model
        .tiers
        .iter()
        .find(|t| t.max_context_tokens.is_none_or(|v| context <= v))
        .expect("validated last tier")
}
pub fn rates_cost(tokens: &Tokens, rates: &Rates) -> Result<[Decimal; 4]> {
    validate_tokens(tokens)?;
    let rate = |value: Option<&String>, count: u64| -> Result<Decimal> {
        if count == 0 {
            return Ok(Decimal::ZERO);
        }
        Ok(Decimal::from(count)
            * number(
                value.context("Go cache-write duration price is unavailable")?,
                false,
            )?
            / Decimal::from(1_000_000))
    };
    Ok([
        rate(Some(&rates.input), tokens.input)?,
        rate(Some(&rates.output), tokens.output)?,
        rate(Some(&rates.cache_read), tokens.cache_read)?,
        rate(
            rates.cache_write.as_ref(),
            tokens.cache_write - tokens.cache_write1h,
        )? + rate(rates.cache_write1h.as_ref(), tokens.cache_write1h)?,
    ])
}
pub fn quote(
    model: &Model,
    schedule: &Schedule,
    tokens: &Tokens,
    timestamp: i64,
    force_peak: Option<bool>,
) -> Result<(Decimal, Option<PricingContext>, Rates)> {
    validate_tokens(tokens)?;
    let tier = tier(model, tokens);
    let mut ctx = if tier.peak_prices.is_some() {
        context(schedule, timestamp)
    } else {
        None
    };
    if tier.peak_prices.is_some() {
        let c = ctx.as_mut().context("Cannot resolve Go request time")?;
        if let Some(peak) = force_peak {
            c.period = if peak { "peak" } else { "off_peak" }.into();
        }
    }
    let rates = if ctx.as_ref().is_some_and(|c| c.period == "peak") {
        tier.peak_prices.as_ref().unwrap()
    } else {
        &tier.prices
    };
    let cost = rates_cost(tokens, rates)?.into_iter().sum();
    Ok((cost, ctx, rates.clone()))
}
pub fn install(prices: &mut Vec<Price>, catalog: &Catalog) {
    prices.retain(|p| {
        p.provider.as_deref() != Some("opencode-go")
            || p.source != "opencode_go" && find(catalog, &p.model_id).is_none()
    });
    let hash = revision(catalog);
    let checked = chrono::NaiveDate::parse_from_str(&catalog.checked_at, "%Y-%m-%d")
        .expect("validated date")
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp();
    for model in &catalog.models {
        let rates = &model.tiers[0].prices;
        prices.push(Price {
            provider: Some("opencode-go".into()),
            model_id: model.id.clone(),
            display_name: model.name.clone(),
            currency: "USD".into(),
            input_cost_per_million: rates.input.clone(),
            output_cost_per_million: rates.output.clone(),
            cache_read_cost_per_million: rates.cache_read.clone(),
            cache_creation_cost_per_million: rates
                .cache_write
                .clone()
                .unwrap_or_else(|| "0".into()),
            source: "opencode_go".into(),
            source_key: Some(catalog.source_url.clone()),
            fetched_at: checked,
            pricing_rule: None,
            billing_basis: Some("subscription_usage".into()),
            go_tariff: Some(Tariff {
                catalog_version: catalog.version.clone(),
                revision: hash.clone(),
                model: model.clone(),
                peak_schedule: catalog.peak_schedule.clone(),
                selected_rates: None,
            }),
        });
    }
}
pub fn selected(
    price: &Price,
    tokens: &Tokens,
    timestamp: i64,
) -> Result<Option<(Price, Option<PricingContext>)>> {
    let Some(tariff) = &price.go_tariff else {
        return Ok(None);
    };
    let Ok((_, context, rates)) = quote(
        &tariff.model,
        &tariff.peak_schedule,
        tokens,
        timestamp,
        None,
    ) else {
        return Ok(None);
    };
    let mut price = price.clone();
    price.input_cost_per_million = rates.input.clone();
    price.output_cost_per_million = rates.output.clone();
    price.cache_read_cost_per_million = rates.cache_read.clone();
    price.cache_creation_cost_per_million = rates.cache_write.clone().unwrap_or_else(|| "0".into());
    price.go_tariff.as_mut().unwrap().selected_rates = Some(rates);
    Ok(Some((price, context)))
}
pub fn allowance_percent(
    cost: Decimal,
    limit: Option<Decimal>,
    fraction: Decimal,
) -> Result<Decimal> {
    let Some(limit) = limit else {
        return Ok(Decimal::ZERO);
    };
    cost.checked_div(limit)
        .and_then(|v| v.checked_div(fraction))
        .and_then(|v| v.checked_mul(Decimal::from(100)))
        .context("Go estimate exceeds supported precision")
}
fn read_file(path: &Path) -> Result<Option<Vec<u8>>> {
    use std::io::Read;
    let file = match fs::File::open(path) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= MAX_BYTES, "Go file is too large");
    Ok(Some(bytes))
}
pub fn file_info(path: &Path) -> Value {
    match read_file(path) {
        Ok(Some(bytes)) => {
            let revision = crate::harness::files::hash(&bytes);
            let parsed = (|| -> Result<Catalog> {
                anyhow::ensure!(bytes.len() <= MAX_BYTES, "Go file is too large");
                let catalog = serde_json::from_slice(&bytes)?;
                validate(&catalog)?;
                Ok(catalog)
            })();
            match parsed {
                Ok(catalog) => json!({"revision":revision,"catalog":catalog,"error":null}),
                Err(_) => {
                    json!({"revision":revision,"catalog":null,"error":"Local Go file is invalid; active rules were preserved"})
                }
            }
        }
        Ok(None) => {
            json!({"revision":null,"catalog":null,"error":null})
        }
        Err(_) => json!({"revision":null,"catalog":null,"error":"Cannot read the local Go file"}),
    }
}
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result.context("Cannot save the local Go rules file")
}
pub fn info(store: &Store, path: &Path) -> Result<Value> {
    let catalog = load(store)?;
    Ok(json!({"revision":revision(&catalog),"catalog":catalog,"path":path,"file":file_info(path)}))
}
pub fn save(store: &Store, path: &Path, args: Value) -> Result<Value> {
    let catalog: Catalog =
        serde_json::from_value(args["catalog"].clone()).context("Invalid Go catalog JSON")?;
    validate(&catalog)?;
    anyhow::ensure!(
        args["expectedRevision"].as_str() == Some(revision(&load(store)?).as_str()),
        "Go rules changed; reload before saving"
    );
    let previous = read_file(path)?;
    let old_hash = previous.as_ref().map(|v| crate::harness::files::hash(v));
    anyhow::ensure!(
        args["expectedFileRevision"].as_str() == old_hash.as_deref(),
        "Go file changed; reload before saving"
    );
    let bytes = serde_json::to_vec_pretty(&catalog)?;
    write_atomic(path, &bytes)?;
    if let Err(error) = store.set_meta(META, &catalog) {
        if let Some(bytes) = previous {
            write_atomic(path, &bytes)?;
        } else {
            fs::remove_file(path)?;
        }
        return Err(error);
    }
    info(store, path)
}

#[cfg(test)]
mod tests;
