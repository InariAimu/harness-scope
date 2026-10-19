use crate::model::{
    Cost, CurrencyConversion, ParsedUsage, Price, PricingContext, PricingSettings, Tokens,
    UsageRecord,
};
use anyhow::Result;
use rust_decimal::Decimal;
use std::str::FromStr;

pub mod deepseek;
pub const CURRENCIES: &[&str] = &["USD", "CNY", "EUR", "JPY", "TWD", "TRY"];

pub fn validate_settings(settings: &PricingSettings) -> Result<()> {
    for (currency, rate) in &settings.exchange_rates {
        anyhow::ensure!(
            CURRENCIES.contains(&currency.as_str()) && currency != "USD",
            "Unsupported pricing currency"
        );
        validate_rate(rate)?;
    }
    anyhow::ensure!(
        settings.holiday_calendars.len() <= 100,
        "Too many holiday calendars"
    );
    for (year, dates) in &settings.holiday_calendars {
        let y: i32 = year.parse()?;
        anyhow::ensure!(
            (2000..=2100).contains(&y) && year == &format!("{y:04}") && dates.len() <= 366,
            "Invalid holiday year"
        );
        let mut unique = std::collections::HashSet::new();
        for date in dates {
            let d = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")?;
            anyhow::ensure!(
                d.format("%Y").to_string() == *year
                    && d.format("%Y-%m-%d").to_string() == *date
                    && unique.insert(date),
                "Invalid or duplicate holiday date"
            );
        }
    }
    Ok(())
}

pub(crate) fn validate_rate(rate: &str) -> Result<Decimal> {
    let value = Decimal::from_str(rate)?;
    anyhow::ensure!(
        value >= Decimal::new(1, 9) && value <= Decimal::from(1_000_000_000u64),
        "Invalid pricing exchange rate"
    );
    Ok(value)
}

pub fn builtin() -> Vec<Price> {
    let mut prices = serde_json::from_str(include_str!("../resources/builtin-pricing.json"))
        .expect("vendored prices");
    deepseek::install(&mut prices);
    prices
}
/// Freeze estimates until usage changes; native reported amounts remain authoritative.
pub fn resolve(parsed: &ParsedUsage, old: Option<&UsageRecord>, prices: &[Price]) -> Result<Cost> {
    resolve_with_settings(parsed, old, prices, &deepseek::default_settings())
}
pub fn resolve_with_settings(
    parsed: &ParsedUsage,
    old: Option<&UsageRecord>,
    prices: &[Price],
    settings: &PricingSettings,
) -> Result<Cost> {
    if let Some(amount) = &parsed.reported_cost {
        return Ok(Cost {
            amount_usd: Some(amount.clone()),
            method: "reported".into(),
            ..Default::default()
        });
    }
    if let Some(old) = old {
        if old.tokens == parsed.tokens && old.model == parsed.model && old.cost.amount_usd.is_some()
        {
            return Ok(old.cost.clone());
        }
    }
    if let Some(price) = old
        .filter(|old| old.model == parsed.model)
        .and_then(|old| old.cost.snapshot.as_ref())
    {
        if old.is_some_and(|r| r.cost.amount_usd.is_some()) {
            let cost = &old.unwrap().cost;
            return estimate_converted(
                &parsed.tokens,
                price,
                cost.conversion
                    .as_ref()
                    .and_then(|c| c.units_per_usd.as_deref()),
                cost.conversion.as_ref().map_or(0, |c| c.configured_at),
                cost.context.clone(),
            );
        }
    }
    find(prices, &parsed.model)
        .map(|price| estimate_at(&parsed.tokens, price, parsed.occurred_at, settings))
        .unwrap_or_else(|| Ok(Cost::default()))
}
pub fn validate(price: &Price) -> Result<()> {
    anyhow::ensure!(!price.model_id.trim().is_empty(), "Empty pricing model");
    anyhow::ensure!(
        CURRENCIES.contains(&price.currency.as_str()),
        "Unsupported pricing currency"
    );
    anyhow::ensure!(
        price
            .pricing_rule
            .as_deref()
            .is_none_or(|rule| rule == deepseek::RULE
                && price.currency == "CNY"
                && deepseek::canonical(&price.model_id) == Some(price.model_id.as_str())),
        "Invalid pricing rule"
    );
    for s in [
        &price.input_cost_per_million,
        &price.output_cost_per_million,
        &price.cache_read_cost_per_million,
        &price.cache_creation_cost_per_million,
    ] {
        let d = Decimal::from_str(s)?;
        anyhow::ensure!(
            d >= Decimal::ZERO && d < Decimal::from(1_000_000_000u64),
            "Invalid unit price"
        );
    }
    Ok(())
}
pub fn normalize(model: &str) -> String {
    let model = model
        .rsplit('/')
        .next()
        .unwrap_or(model)
        .split(':')
        .next()
        .unwrap_or(model)
        .trim()
        .to_lowercase()
        .replace('@', "-");
    let model = model.trim_end_matches("[1m]").trim().to_owned();
    if model.starts_with("claude-") {
        model.replace('.', "-")
    } else {
        model
    }
}
pub fn find<'a>(prices: &'a [Price], model: &str) -> Option<&'a Price> {
    if let Some(canonical) = deepseek::canonical(model) {
        return prices.iter().find(|p| p.model_id == canonical);
    }
    let candidates = crate::pricing_match::model_pricing_candidates(model);
    for c in &candidates {
        if let Some(p) = prices.iter().find(|p| p.model_id.eq_ignore_ascii_case(c)) {
            return Some(p);
        }
    }
    for c in &candidates {
        if crate::pricing_match::should_try_pricing_prefix_match(c) {
            if let Some(p) = prices
                .iter()
                .filter(|p| p.model_id.to_lowercase().starts_with(&format!("{c}-")))
                .min_by_key(|p| p.model_id.len())
            {
                return Some(p);
            }
        }
    }
    None
}
pub fn estimate(tokens: &Tokens, price: &Price) -> Result<Cost> {
    estimate_converted(tokens, price, None, 0, None)
}

pub fn estimate_at(
    tokens: &Tokens,
    price: &Price,
    timestamp: i64,
    settings: &PricingSettings,
) -> Result<Cost> {
    let mut selected = price.clone();
    let context = if price.pricing_rule.as_deref() == Some(deepseek::RULE) {
        let Some(context) = deepseek::context(timestamp, settings) else {
            return Ok(Cost::default());
        };
        if context.period == "peak" {
            for value in [
                &mut selected.input_cost_per_million,
                &mut selected.output_cost_per_million,
                &mut selected.cache_read_cost_per_million,
            ] {
                *value = (Decimal::from_str(value)? * Decimal::TWO)
                    .normalize()
                    .to_string();
            }
        }
        Some(context)
    } else {
        None
    };
    estimate_converted(
        tokens,
        &selected,
        settings
            .exchange_rates
            .get(&selected.currency)
            .map(String::as_str),
        settings.updated_at,
        context,
    )
}

pub fn components(tokens: &Tokens, price: &Price) -> Result<[Decimal; 4]> {
    validate(price)?;
    let unit = |s: &str| Decimal::from_str(s);
    let cache = Decimal::from(tokens.cache_write.saturating_sub(tokens.cache_write1h))
        + Decimal::from(tokens.cache_write1h) * Decimal::new(16, 1);
    Ok([
        Decimal::from(tokens.input) * unit(&price.input_cost_per_million)?,
        Decimal::from(tokens.output) * unit(&price.output_cost_per_million)?,
        Decimal::from(tokens.cache_read) * unit(&price.cache_read_cost_per_million)?,
        cache * unit(&price.cache_creation_cost_per_million)?,
    ]
    .map(|amount| amount / Decimal::from(1_000_000)))
}

fn estimate_converted(
    tokens: &Tokens,
    price: &Price,
    rate: Option<&str>,
    configured_at: i64,
    context: Option<PricingContext>,
) -> Result<Cost> {
    let original: Decimal = components(tokens, price)?.into_iter().sum();
    let conversion = (price.currency != "USD").then(|| CurrencyConversion {
        currency: price.currency.clone(),
        original_amount: original.normalize().to_string(),
        units_per_usd: rate.map(str::to_string),
        configured_at,
    });
    let amount = if price.currency == "USD" {
        Some(original)
    } else {
        rate.map(validate_rate)
            .transpose()?
            .map(|rate| original / rate)
    };
    Ok(Cost {
        amount_usd: amount.map(|v| v.normalize().to_string()),
        method: if amount.is_some() {
            "estimated"
        } else {
            "unknown"
        }
        .into(),
        price_source: price.source.clone(),
        pricing_model: Some(price.model_id.clone()),
        priced_at: chrono::Utc::now().timestamp(),
        snapshot: Some(price.clone()),
        conversion,
        context,
        rule_version: price
            .pricing_rule
            .clone()
            .unwrap_or_else(|| "flat-price-v2".into()),
    })
}
