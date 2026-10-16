use crate::model::{Cost, Price, Tokens};
use anyhow::Result;
use rust_decimal::Decimal;
use std::str::FromStr;

pub fn builtin() -> Vec<Price> {
    serde_json::from_str(include_str!("../resources/builtin-pricing.json"))
        .expect("vendored prices")
}
pub fn validate(price: &Price) -> Result<()> {
    anyhow::ensure!(!price.model_id.trim().is_empty(), "Empty pricing model");
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
    validate(price)?;
    let unit = |s: &str| Decimal::from_str(s);
    let cache = Decimal::from(tokens.cache_write.saturating_sub(tokens.cache_write1h))
        + Decimal::from(tokens.cache_write1h) * Decimal::new(16, 1);
    let amount = (Decimal::from(tokens.input) * unit(&price.input_cost_per_million)?
        + Decimal::from(tokens.output) * unit(&price.output_cost_per_million)?
        + Decimal::from(tokens.cache_read) * unit(&price.cache_read_cost_per_million)?
        + cache * unit(&price.cache_creation_cost_per_million)?)
        / Decimal::from(1_000_000);
    Ok(Cost {
        amount_usd: Some(amount.normalize().to_string()),
        method: "estimated".into(),
        price_source: price.source.clone(),
        pricing_model: Some(price.model_id.clone()),
        priced_at: chrono::Utc::now().timestamp(),
        snapshot: Some(price.clone()),
        ..Default::default()
    })
}
