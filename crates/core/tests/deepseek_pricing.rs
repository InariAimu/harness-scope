use chrono::DateTime;
use harness_scope_core::{
    model::{Cost, ParsedUsage, Price, PricingSettings, PushRequest, Tokens, UsageRecord},
    pricing::{self, deepseek},
    store::Store,
    sync, Engine,
};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::str::FromStr;

fn ts(value: &str) -> i64 {
    DateTime::parse_from_rfc3339(value).unwrap().timestamp()
}
fn settings(rate: &str) -> PricingSettings {
    let mut s = deepseek::default_settings();
    s.exchange_rates.insert("CNY".into(), rate.into());
    s.updated_at = 1792339200;
    s
}
fn tokens() -> Tokens {
    Tokens {
        input: 1_000_000,
        output: 1_000_000,
        cache_read: 1_000_000,
        reasoning: 400_000,
        ..Default::default()
    }
}
fn parsed(model: &str, time: &str) -> ParsedUsage {
    ParsedUsage {
        id: "pricing-fixture".into(),
        session_id: "session".into(),
        native_event_id: "event".into(),
        occurred_at: ts(time),
        model: model.into(),
        tokens: tokens(),
        reported_cost: None,
        timing: None,
    }
}
fn record(e: &Engine, p: &ParsedUsage, cost: Cost) -> UsageRecord {
    UsageRecord {
        device_id: e.lock().unwrap().device_id().unwrap(),
        id: p.id.clone(),
        harness: "dsh".into(),
        parser_version: "fixture".into(),
        session_id: Some(p.session_id.clone()),
        native_event_id: Some(p.native_event_id.clone()),
        occurred_at: p.occurred_at,
        model: p.model.clone(),
        grain: "event".into(),
        count: 1,
        tokens: p.tokens.clone(),
        cost,
        record_source: "session".into(),
        revision: 1,
        deleted: false,
        status_code: None,
        legacy_date: None,
        legacy_timezone: None,
    }
}
fn save_rate(e: &Engine, value: &str) -> Value {
    e.execute(
        "save_pricing_settings",
        json!({"patch":{"exchangeRates":{"CNY":value}}}),
    )
    .unwrap()
}

#[test]
fn aliases_price_canonically_without_collapsing_unrelated_models() {
    let prices = pricing::builtin();
    for model in [
        "deepseek-flash",
        "DeepSeek-V4.1-Flash-1017",
        "deepseek-v4-flash",
        "deepseek-v4-flash-vision-exp",
        "deepseek-v4-flash-0731",
        "deepseek-v4-1-flash",
        "deepseek/deepseek-v4.1-flash[1m]",
        "deepseek-v4.1-flash-high",
        "claude-deepseek-v4.1-flash",
    ] {
        assert_eq!(
            pricing::find(&prices, model).unwrap().model_id,
            "deepseek-flash",
            "{model}"
        );
    }
    for model in [
        "deepseek-v4-pro",
        "DeepSeek-V4-Pro-0813",
        "provider/deepseek-v4-pro-20260813",
    ] {
        assert_eq!(
            pricing::find(&prices, model).unwrap().model_id,
            "deepseek-v4-pro"
        );
    }
    for model in [
        "deepseek-chat",
        "deepseek-reasoner",
        "deepseek-v3.2",
        "deepseek-v4-prototype",
        "deepseek-v4-flash-custom",
        "gpt-5.6-luna",
    ] {
        assert_eq!(deepseek::canonical(model), None, "{model}");
    }
}

#[test]
fn peak_boundaries_weekends_holidays_and_timezones_follow_beijing() {
    let s = settings("6.68249010");
    for (time, period) in [
        ("2026-10-19T08:59:59+08:00", "off_peak"),
        ("2026-10-19T09:00:00+08:00", "peak"),
        ("2026-10-19T11:59:59+08:00", "peak"),
        ("2026-10-19T12:00:00+08:00", "off_peak"),
        ("2026-10-19T13:59:59+08:00", "off_peak"),
        ("2026-10-19T14:00:00+08:00", "peak"),
        ("2026-10-19T17:59:59+08:00", "peak"),
        ("2026-10-19T18:00:00+08:00", "off_peak"),
        ("2026-10-01T10:00:00+08:00", "off_peak"),
        ("2026-09-25T10:00:00+08:00", "off_peak"),
        ("2026-10-10T10:00:00+08:00", "off_peak"), // make-up Saturday
        ("2026-10-18T10:00:00+08:00", "off_peak"),
        ("2026-10-19T01:00:00Z", "peak"),
        ("2026-10-18T21:00:00-04:00", "peak"),
    ] {
        assert_eq!(
            deepseek::context(ts(time), &s).unwrap().period,
            period,
            "{time}"
        );
    }
    assert!(deepseek::context(ts("2026-09-10T11:59:59+08:00"), &s).is_none());
    assert!(deepseek::context(ts("2026-09-10T12:00:00+08:00"), &s).is_some());
    assert!(deepseek::context(ts("2027-01-01T10:00:00+08:00"), &s).is_none());
    let mut next = s;
    next.holiday_calendars
        .insert("2027".into(), vec!["2027-01-01".into()]);
    assert_eq!(
        deepseek::context(ts("2027-01-01T10:00:00+08:00"), &next)
            .unwrap()
            .period,
        "off_peak"
    );
}

#[test]
fn original_currency_and_exact_decimal_fx_are_preserved_without_double_counting() {
    let s = settings("6.68249010");
    let prices = pricing::builtin();
    for (model, native) in [("deepseek-flash", "5.02"), ("deepseek-v4-pro", "18.15")] {
        let p = pricing::find(&prices, model).unwrap();
        for (time, multiplier) in [
            ("2026-10-19T08:00:00+08:00", 1),
            ("2026-10-19T09:00:00+08:00", 2),
        ] {
            let c = pricing::estimate_at(&tokens(), p, ts(time), &s).unwrap();
            let amount = Decimal::from_str(native).unwrap() * Decimal::from(multiplier);
            assert_eq!(
                Decimal::from_str(c.amount_usd.as_deref().unwrap()).unwrap(),
                amount / Decimal::from_str("6.68249010").unwrap()
            );
            let conversion = c.conversion.as_ref().unwrap();
            assert_eq!(conversion.original_amount, amount.normalize().to_string());
            assert_eq!(conversion.units_per_usd.as_deref(), Some("6.68249010"));
            assert_eq!(c.pricing_model.as_deref(), Some(model));
            assert_eq!(c.rule_version, deepseek::RULE);
            assert_eq!(c.snapshot.as_ref().unwrap().currency, "CNY");
        }
    }
    let p = pricing::find(&prices, "deepseek-flash").unwrap();
    let c = pricing::estimate_at(
        &tokens(),
        p,
        ts("2026-10-19T08:00:00+08:00"),
        &deepseek::default_settings(),
    )
    .unwrap();
    assert!(c.amount_usd.is_none());
    assert_eq!(c.conversion.unwrap().original_amount, "5.02");
}

#[test]
fn latest_rate_fills_unknowns_but_history_and_corrected_usage_keep_the_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("scope.db");
    let e = Engine::open(&path).unwrap();
    let p = parsed("deepseek-v4.1-flash", "2026-10-19T10:00:00+08:00");
    let r = record(&e, &p, Cost::default());
    e.lock().unwrap().upsert(r, "fixture").unwrap();
    save_rate(&e, "6.68249010");
    let old = e.lock().unwrap().record(&p.id).unwrap().unwrap();
    assert!(old.cost.amount_usd.is_some());
    assert_eq!(old.model, p.model);
    save_rate(&e, "7");
    let new = e.lock().unwrap().record(&p.id).unwrap().unwrap();
    assert_eq!(new, old);
    let resolved =
        pricing::resolve_with_settings(&p, Some(&old), &pricing::builtin(), &settings("8"))
            .unwrap();
    assert_eq!(resolved, old.cost);
    let mut corrected = p.clone();
    corrected.tokens.output += 1_000_000;
    let corrected_cost =
        pricing::resolve_with_settings(&corrected, Some(&old), &pricing::builtin(), &settings("8"))
            .unwrap();
    assert_eq!(corrected_cost.snapshot, old.cost.snapshot);
    assert_eq!(corrected_cost.context, old.cost.context);
    assert_eq!(
        corrected_cost
            .conversion
            .as_ref()
            .unwrap()
            .units_per_usd
            .as_deref(),
        Some("6.68249010")
    );
    let fresh =
        pricing::resolve_with_settings(&p, None, &pricing::builtin(), &settings("7")).unwrap();
    assert_eq!(
        fresh.conversion.unwrap().units_per_usd.as_deref(),
        Some("7")
    );
    drop(e);
    let e = Engine::open(&path).unwrap();
    assert_eq!(
        e.lock().unwrap().pricing_settings().unwrap().exchange_rates["CNY"],
        "7"
    );
    assert_eq!(
        e.execute("reprice_usage", json!({"model":"deepseek-flash"}))
            .unwrap(),
        1
    );
    let updated = e.lock().unwrap().record(&p.id).unwrap().unwrap();
    assert_eq!(
        updated.cost.conversion.unwrap().units_per_usd.as_deref(),
        Some("7")
    );
    assert!(updated.revision > old.revision);
    assert_eq!(e.execute("reprice_usage", json!({})).unwrap(), 0);
}

#[test]
fn reprice_preserves_reported_migrated_deleted_and_remote_usage() {
    let tmp = tempfile::tempdir().unwrap();
    let e = Engine::open(&tmp.path().join("scope.db")).unwrap();
    save_rate(&e, "6.68249010");
    let p = parsed("deepseek-v4-flash", "2026-10-19T10:00:00+08:00");
    for (id, source, method, deleted) in [
        ("reported", "session", "reported", false),
        ("migration", "migration", "estimated", false),
        ("deleted", "session", "estimated", true),
    ] {
        let mut r = record(
            &e,
            &p,
            Cost {
                amount_usd: Some("100".into()),
                method: method.into(),
                ..Default::default()
            },
        );
        r.id = id.into();
        r.record_source = source.into();
        r.deleted = deleted;
        e.lock().unwrap().upsert(r, "fixture").unwrap();
    }
    assert_eq!(e.execute("reprice_usage", json!({})).unwrap(), 0);
    for r in e.lock().unwrap().local_records().unwrap() {
        assert_eq!(r.cost.amount_usd.as_deref(), Some("100"));
    }
    let old = record(
        &e,
        &p,
        Cost {
            amount_usd: Some("1".into()),
            method: "estimated".into(),
            ..Default::default()
        },
    );
    let mut native = p;
    native.reported_cost = Some("2".into());
    let cost =
        pricing::resolve_with_settings(&native, Some(&old), &pricing::builtin(), &settings("7"))
            .unwrap();
    assert_eq!(cost.amount_usd.as_deref(), Some("2"));
    assert_eq!(cost.method, "reported");
    assert!(cost.conversion.is_none());
}

#[test]
fn imported_prices_cannot_replace_official_rules_and_other_currencies_work() {
    let tmp = tempfile::tempdir().unwrap();
    let e = Engine::open(&tmp.path().join("scope.db")).unwrap();
    let mut p = pricing::find(&pricing::builtin(), "deepseek-flash")
        .unwrap()
        .clone();
    p.source = "models_dev".into();
    p.currency = "USD".into();
    p.pricing_rule = None;
    p.input_cost_per_million = "999".into();
    assert_eq!(e.execute("save_prices", json!({"entries":[p]})).unwrap(), 0);
    let prices = e.lock().unwrap().prices().unwrap();
    assert_eq!(
        pricing::find(&prices, "deepseek-v4-flash").unwrap().source,
        "deepseek_official"
    );
    let custom: Price = serde_json::from_value(json!({"modelId":"test-eur","displayName":"EUR model","currency":"EUR","inputCostPerMillion":"2","outputCostPerMillion":"3","cacheReadCostPerMillion":"1","cacheCreationCostPerMillion":"0","source":"manual","sourceKey":null,"fetchedAt":0})).unwrap();
    let mut s = settings("7");
    s.exchange_rates.insert("EUR".into(), "0.8".into());
    let cost = pricing::estimate_at(&tokens(), &custom, 0, &s).unwrap();
    assert_eq!(cost.amount_usd.as_deref(), Some("7.5"));
    assert_eq!(cost.conversion.unwrap().currency, "EUR");
}

#[test]
fn query_and_sync_preserve_original_models_native_currency_and_breakdowns() {
    let tmp = tempfile::tempdir().unwrap();
    let e = Engine::open(&tmp.path().join("scope.db")).unwrap();
    save_rate(&e, "6.68249010");
    let p = parsed("DeepSeek-V4.1-Flash-1017", "2026-10-19T10:00:00+08:00");
    let cost =
        pricing::resolve_with_settings(&p, None, &pricing::builtin(), &settings("6.68249010"))
            .unwrap();
    let r = record(&e, &p, cost);
    sync::validate_record(&r).unwrap();
    e.lock().unwrap().upsert(r.clone(), "fixture").unwrap();
    let logs = e
        .execute("usage_query", json!({"kind":"logs","scope":"local"}))
        .unwrap();
    let log = &logs["data"][0];
    assert_eq!(log["model"], p.model);
    assert_eq!(log["pricingModel"], "deepseek-flash");
    assert_eq!(log["costConverted"], true);
    assert_eq!(log["costConversion"]["originalAmount"], "10.04");
    assert_eq!(log["pricingContext"]["period"], "peak");
    let input = Decimal::from_str(log["inputCostUsd"].as_str().unwrap()).unwrap();
    assert_eq!(
        input,
        Decimal::from(2) / Decimal::from_str("6.68249010").unwrap()
    );
    let models = e
        .execute("usage_query", json!({"kind":"models","scope":"local"}))
        .unwrap();
    assert_eq!(models[0]["model"], p.model);
    assert_eq!(models[0]["costConverted"], true);
    let server = Store::open(&tmp.path().join("server.db")).unwrap();
    sync::push(
        &server,
        PushRequest {
            protocol_version: 1,
            device_id: r.device_id.clone(),
            device_name: "fixture".into(),
            records: vec![r.clone()],
        },
    )
    .unwrap();
    let changes = sync::changes(&server, 0, 100).unwrap();
    assert_eq!(changes.changes[0].record, r);
    let peer = Store::open(&tmp.path().join("peer.db")).unwrap();
    sync::apply_changes(&peer, &changes).unwrap();
    assert_eq!(peer.records("all").unwrap()[0], r);
    let mut forged = r;
    forged.cost.conversion.as_mut().unwrap().units_per_usd = Some("0".into());
    assert!(sync::validate_record(&forged).is_err());
}

#[test]
fn invalid_settings_are_atomic_and_old_usd_prices_remain_readable() {
    let tmp = tempfile::tempdir().unwrap();
    let e = Engine::open(&tmp.path().join("scope.db")).unwrap();
    let before = save_rate(&e, "6.68249010");
    for patch in [
        json!({"exchangeRates":{"CNY":"0"}}),
        json!({"exchangeRates":{"CNY":"NaN"}}),
        json!({"exchangeRates":{"CNY":"-1"}}),
        json!({"exchangeRates":{"USD":"7"}}),
        json!({"exchangeRates":{"CNY":"1e50"}}),
        json!({"holidayCalendars":{"2027":["2026-01-01"]}}),
        json!({"holidayCalendars":{"2027":["2027-02-30"]}}),
        json!({"exchangeRates":{"CNY":"7"},"private":"no"}),
    ] {
        assert!(e
            .execute("save_pricing_settings", json!({"patch":patch}))
            .is_err());
        assert_eq!(e.execute("pricing_settings", json!({})).unwrap(), before);
    }
    let mut v =
        serde_json::to_value(pricing::find(&pricing::builtin(), "gpt-5.4").unwrap()).unwrap();
    v.as_object_mut().unwrap().remove("currency");
    let old: Price = serde_json::from_value(v).unwrap();
    assert_eq!(old.currency, "USD");
    assert!(pricing::estimate(&tokens(), &old)
        .unwrap()
        .conversion
        .is_none());
}
