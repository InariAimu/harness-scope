//! Go prices measure subscription allowance consumption, not a per-request cash bill.
use crate::model::{Price, PricingContext};
use chrono::{Datelike, TimeZone, Timelike};

pub const RULE: &str = "opencode-go-deepseek-2026-10-19";
pub const SOURCE: &str = "https://opencode.ai/v2/docs/console/go/";
pub fn install(prices: &mut Vec<Price>) {
    prices.retain(|p| {
        !(p.provider.as_deref() == Some("opencode-go")
            && super::deepseek::canonical(&p.model_id).is_some())
    });
    prices.extend(
        [
            ("deepseek-flash", "DeepSeek Flash", "0.15", "0.60", "0.003"),
            (
                "deepseek-v4-pro",
                "DeepSeek V4 Pro",
                "0.66",
                "1.98",
                "0.022",
            ),
        ]
        .into_iter()
        .map(|(id, name, input, output, cache)| Price {
            provider: Some("opencode-go".into()),
            model_id: id.into(),
            display_name: name.into(),
            currency: "USD".into(),
            input_cost_per_million: input.into(),
            output_cost_per_million: output.into(),
            cache_read_cost_per_million: cache.into(),
            cache_creation_cost_per_million: "0".into(),
            source: "opencode_go".into(),
            source_key: Some(SOURCE.into()),
            fetched_at: 1792339200,
            pricing_rule: Some(RULE.into()),
            billing_basis: Some("subscription_usage".into()),
        }),
    );
}
pub fn context(timestamp: i64) -> Option<PricingContext> {
    let utc = chrono::Utc.timestamp_opt(timestamp, 0).single()?;
    if timestamp <= 0 {
        return None;
    }
    let minute = utc.hour() * 60 + utc.minute();
    let peak = utc.weekday().num_days_from_monday() < 5
        && ((60..240).contains(&minute) || (360..600).contains(&minute));
    Some(PricingContext {
        period: if peak { "peak" } else { "off_peak" }.into(),
        timestamp,
        time_basis: "log_timestamp".into(),
        calendar_version: "UTC-weekdays".into(),
    })
}
