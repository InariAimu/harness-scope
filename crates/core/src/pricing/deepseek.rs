use crate::model::{Price, PricingContext, PricingSettings};
use chrono::{Datelike, TimeZone, Timelike};

pub const RULE: &str = "deepseek-cn-2026-09-10";
pub const SOURCE: &str = "https://api-docs.deepseek.com/zh-cn/quick_start/pricing/";
// https://www.gov.cn/zhengce/content/202511/content_7047090.htm
// Weekends remain off-peak even when they are make-up working days.
pub fn default_settings() -> PricingSettings {
    let mut settings = PricingSettings::default();
    let mut dates = Vec::new();
    for (month, start, end) in [
        (1, 1, 3),
        (2, 15, 23),
        (4, 4, 6),
        (5, 1, 5),
        (6, 19, 21),
        (9, 25, 27),
        (10, 1, 7),
    ] {
        dates.extend((start..=end).map(|day| format!("2026-{month:02}-{day:02}")));
    }
    settings.holiday_calendars.insert("2026".into(), dates);
    settings
}

/// Only recognized Flash / V4 Pro families; V3, chat and reasoner stay distinct.
pub fn canonical(model: &str) -> Option<&'static str> {
    for candidate in crate::pricing_match::model_pricing_candidates(model) {
        let base = candidate
            .rsplit_once('-')
            .filter(|(_, suffix)| {
                matches!(suffix.len(), 4 | 8) && suffix.bytes().all(|b| b.is_ascii_digit())
            })
            .map_or(candidate.as_str(), |(base, _)| base);
        match base {
            "deepseek-flash"
            | "deepseek-v4-flash"
            | "deepseek-v4-flash-vision-exp"
            | "deepseek-v4.1-flash"
            | "deepseek-v4-1-flash" => return Some("deepseek-flash"),
            "deepseek-v4-pro" => return Some("deepseek-v4-pro"),
            _ => {}
        }
    }
    None
}

pub fn prices() -> Vec<Price> {
    [
        ("deepseek-flash", "DeepSeek Flash", "1", "4", "0.02"),
        ("deepseek-v4-pro", "DeepSeek V4 Pro", "4.5", "13.5", "0.15"),
    ]
    .into_iter()
    .map(|(id, name, input, output, hit)| Price {
        go_tariff: None,
        provider: Some("deepseek-official".into()),
        billing_basis: None,
        model_id: id.into(),
        display_name: name.into(),
        currency: "CNY".into(),
        input_cost_per_million: input.into(),
        output_cost_per_million: output.into(),
        cache_read_cost_per_million: hit.into(),
        cache_creation_cost_per_million: "0".into(),
        source: "deepseek_official".into(),
        source_key: Some(SOURCE.into()),
        fetched_at: 1792339200,
        pricing_rule: Some(RULE.into()),
    })
    .collect()
}

pub fn install(prices: &mut Vec<Price>) {
    prices.retain(|p| {
        canonical(&p.model_id).is_none()
            || p.provider.is_none() && matches!(p.source.as_str(), "models_dev" | "manual")
            || p.provider
                .as_deref()
                .is_some_and(|id| id != "deepseek-official")
    });
    prices.extend(self::prices());
}

pub fn context(timestamp: i64, settings: &PricingSettings) -> Option<PricingContext> {
    let local = chrono_tz::Asia::Shanghai
        .timestamp_opt(timestamp, 0)
        .single()?;
    // Do not apply the current tariff to older usage or guess future holiday calendars.
    if timestamp < 1_789_012_800 {
        return None;
    } // 2026-09-10 12:00 +08:00
    let year = local.year().to_string();
    let holidays = settings.holiday_calendars.get(&year)?;
    let holiday = holidays
        .iter()
        .any(|d| *d == local.format("%Y-%m-%d").to_string());
    let minute = local.hour() * 60 + local.minute();
    let peak = local.weekday().num_days_from_monday() < 5
        && !holiday
        && ((540..720).contains(&minute) || (840..1080).contains(&minute));
    Some(PricingContext {
        period: if peak { "peak" } else { "off_peak" }.into(),
        timestamp,
        time_basis: "log_timestamp".into(),
        calendar_version: format!(
            "CN-{year}:{}",
            crate::harness::files::hash(&serde_json::to_vec(holidays).ok()?)
        ),
    })
}
