use crate::model::{ParsedUsage, Price};
use anyhow::Result;
use rust_decimal::Decimal;
use std::str::FromStr;

pub(super) fn validate(price: &Price) -> Result<()> {
    let Some(tariff) = &price.request_tariff else {
        return Ok(());
    };
    anyhow::ensure!(
        price.provider.is_some() && price.go_tariff.is_none() && price.pricing_rule.is_none(),
        "Request tariffs require an explicit provider and cannot overlap managed tariffs"
    );
    anyhow::ensure!(
        !tariff.version.is_empty()
            && tariff.version.len() <= 100
            && !tariff.tiers.is_empty()
            && tariff.tiers.len() <= 50,
        "Invalid request tariff version/tiers"
    );
    let mut ids = std::collections::HashSet::new();
    for t in &tariff.tiers {
        anyhow::ensure!(
            !t.id.is_empty() && t.id.len() <= 100 && ids.insert(&t.id),
            "Invalid or duplicate request tier ID"
        );
        anyhow::ensure!(
            t.speed
                .as_deref()
                .is_none_or(|s| ["fast", "standard"].contains(&s))
                && t.above_prompt_tokens <= 1_000_000_000_000,
            "Invalid request tier condition"
        );
        anyhow::ensure!(
            t.effective_from
                .zip(t.effective_until)
                .is_none_or(|(a, b)| a < b),
            "Invalid tariff date range"
        );
        for s in [
            &t.input_cost_per_million,
            &t.output_cost_per_million,
            &t.cache_read_cost_per_million,
            &t.cache_creation_cost_per_million,
        ] {
            let n = Decimal::from_str(s)?;
            anyhow::ensure!(
                n >= Decimal::ZERO && n < Decimal::from(1_000_000_000u64),
                "Invalid request tier price"
            );
        }
    }
    anyhow::ensure!(
        tariff
            .selected_tier
            .as_ref()
            .is_none_or(|id| ids.contains(id)),
        "Unknown selected tariff tier"
    );
    Ok(())
}

pub(super) fn select(price: &Price, parsed: &ParsedUsage) -> Result<Price> {
    validate(price)?;
    let mut selected = price.clone();
    let Some(tariff) = &price.request_tariff else {
        return Ok(selected);
    };
    // Model reference prices cannot claim a supplier-specific rule when the
    // log does not identify that supplier. A missing speed is never 'standard'.
    anyhow::ensure!(
        price.provider.as_deref().map(crate::providers::pricing_id)
            == parsed.provider.as_deref().map(crate::providers::pricing_id),
        "Request tariff provider mismatch"
    );
    let speed = parsed.request_metadata.as_ref().and_then(|m| m.speed());
    let prompt = parsed
        .tokens
        .input
        .saturating_add(parsed.tokens.cache_read)
        .saturating_add(parsed.tokens.cache_write);
    let chosen = tariff
        .tiers
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            t.speed.as_deref().is_none_or(|s| Some(s) == speed)
                && (t.above_prompt_tokens == 0 || prompt > t.above_prompt_tokens)
                && t.effective_from.is_none_or(|at| parsed.occurred_at >= at)
                && t.effective_until.is_none_or(|at| parsed.occurred_at < at)
        })
        .max_by_key(|(i, t)| {
            (
                t.speed.is_some(),
                t.above_prompt_tokens,
                std::cmp::Reverse(*i),
            )
        });
    if let Some((_, t)) = chosen {
        selected.input_cost_per_million = t.input_cost_per_million.clone();
        selected.output_cost_per_million = t.output_cost_per_million.clone();
        selected.cache_read_cost_per_million = t.cache_read_cost_per_million.clone();
        selected.cache_creation_cost_per_million = t.cache_creation_cost_per_million.clone();
        selected.request_tariff.as_mut().unwrap().selected_tier = Some(t.id.clone());
    }
    Ok(selected)
}
