use harness_scope_core::{
    harness::{codex::Codex, dsh::Dsh, HarnessAdapter, TimelineIndex},
    model::{ParsedUsage, Price, PushRequest, Source, Tokens, UsageRecord},
    pricing::{self, deepseek, opencode_go},
    store::Store,
    sync, Engine,
};
use serde_json::{json, Value};
use std::path::Path;

fn timestamp(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s).unwrap().timestamp()
}
fn usage(provider: Option<&str>) -> ParsedUsage {
    ParsedUsage {
        model_usage: None,
        request_metadata: None,
        provider: provider.map(str::to_owned),
        id: "fixture".into(),
        session_id: "session".into(),
        native_event_id: "fixture".into(),
        occurred_at: timestamp("2026-10-19T09:30:00+08:00"),
        model: "deepseek-v4.1-flash".into(),
        tokens: Tokens {
            input: 1_000_000,
            output: 1_000_000,
            cache_read: 1_000_000,
            ..Default::default()
        },
        reported_cost: None,
        timing: None,
    }
}
fn reference() -> Price {
    serde_json::from_value(json!({"modelId":"deepseek-v4-flash", "displayName":"Reference", "inputCostPerMillion":"0.2", "outputCostPerMillion":"0.8", "cacheReadCostPerMillion":"0.01", "cacheCreationCostPerMillion":"0", "source":"models_dev", "sourceKey":"deepseek/deepseek-v4-flash", "fetchedAt":1})).unwrap()
}
fn record(e: &Engine, p: &ParsedUsage) -> UsageRecord {
    serde_json::from_value(json!({"deviceId":e.lock().unwrap().device_id().unwrap(), "id":p.id, "provider":p.provider, "harness":"dsh", "parserVersion":"old", "sessionId":p.session_id, "nativeEventId":p.native_event_id, "occurredAt":p.occurred_at, "model":p.model, "grain":"event", "count":1, "tokens":p.tokens, "cost":harness_scope_core::model::Cost::default(), "recordSource":"session", "revision":1, "deleted":false, "statusCode":null, "legacyDate":null, "legacyTimezone":null})).unwrap()
}

#[test]
fn openai_model_override_keeps_other_model_references_and_backfills_unknown_costs() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::open(&tmp.path().join("scope.db")).unwrap();
    let mut sol_reference = reference();
    sol_reference.model_id = "gpt-6.1-sol".into();
    sol_reference.source_key = Some("openai/gpt-6.1-sol".into());
    let mut review_reference = sol_reference.clone();
    review_reference.model_id = "codex-auto-review".into();
    review_reference.source_key = Some("openai/codex-auto-review".into());
    engine
        .execute(
            "save_prices",
            json!({"entries": [sol_reference, review_reference.clone()]}),
        )
        .unwrap();

    let mut sol = usage(Some("openai"));
    sol.model = "gpt-6.1-sol".into();
    let mut pending = record(&engine, &sol);
    pending.harness = "codex".into();
    engine.lock().unwrap().upsert(pending, "fixture").unwrap();

    let mut review_override = review_reference;
    review_override.provider = Some("openai".into());
    review_override.source = "manual".into();
    review_override.source_key = None;
    review_override.input_cost_per_million = "0".into();
    review_override.output_cost_per_million = "0".into();
    review_override.cache_read_cost_per_million = "0".into();
    let mut custom_override = review_override.clone();
    custom_override.provider = Some("custom-route".into());
    engine
        .execute(
            "save_prices",
            json!({"entries": [review_override, custom_override]}),
        )
        .unwrap();

    let store = engine.lock().unwrap();
    let backfilled = store.record(&sol.id).unwrap().unwrap();
    assert_eq!(backfilled.cost.amount_usd.as_deref(), Some("1.01"));
    assert_eq!(
        backfilled.cost.billing_basis.as_deref(),
        Some("model_reference")
    );

    let prices = store.prices().unwrap();
    let settings = store.pricing_settings().unwrap();
    let new_sol = pricing::resolve_with_settings(&sol, None, &prices, &settings).unwrap();
    assert_eq!(new_sol.amount_usd, backfilled.cost.amount_usd);
    assert_eq!(new_sol.price_source, "models_dev");

    let mut review = sol;
    review.model = "codex-auto-review".into();
    let free_review = pricing::resolve_with_settings(&review, None, &prices, &settings).unwrap();
    assert_eq!(free_review.amount_usd.as_deref(), Some("0"));
    assert_eq!(free_review.price_source, "manual");
    assert!(pricing::find_for_provider(&prices, "gpt-6.1-sol", Some("custom-route")).is_none());
}

#[test]
fn provider_tariffs_and_unconfigured_model_references_are_separate() {
    let mut prices = pricing::builtin();
    prices.push(reference());
    let mut settings = deepseek::default_settings();
    settings
        .exchange_rates
        .insert("CNY".into(), "6.68249010".into());
    for (provider, source, currency, basis) in [
        (Some("deepseek-official"), "deepseek_official", "CNY", None),
        (Some("deepseek"), "deepseek_official", "CNY", None),
        (
            Some("opencode-go"),
            "opencode_go",
            "USD",
            Some("subscription_usage"),
        ),
        (
            Some("custom-route"),
            "models_dev",
            "USD",
            Some("model_reference"),
        ),
        (None, "models_dev", "USD", Some("model_reference")),
    ] {
        let p = usage(provider);
        let cost = pricing::resolve_with_settings(&p, None, &prices, &settings).unwrap();
        assert_eq!(cost.price_source, source);
        assert_eq!(cost.snapshot.as_ref().unwrap().currency, currency);
        assert_eq!(cost.billing_basis.as_deref(), basis);
        assert_eq!(p.model, "deepseek-v4.1-flash");
        if provider == Some("opencode-go") {
            assert_eq!(cost.amount_usd.as_deref(), Some("1.506"));
            assert!(cost.conversion.is_none());
            assert_eq!(cost.pricing_model.as_deref(), Some("deepseek-flash"));
        }
        if provider == Some("custom-route") {
            assert_eq!(cost.amount_usd.as_deref(), Some("1.01"));
        }
    }
    assert_eq!(
        pricing::find_for_provider(&pricing::builtin(), "deepseek-flash", Some("custom-route"))
            .unwrap()
            .source,
        "deepseek_official"
    );
    assert!(
        pricing::find_for_provider(&pricing::builtin(), "gpt-5.4", Some("custom-route")).is_none()
    );
    let mut scoped = reference();
    scoped.provider = Some("custom-route".into());
    scoped.source = "manual".into();
    scoped.input_cost_per_million = "3".into();
    prices.push(scoped);
    assert_eq!(
        pricing::find_for_provider(&prices, "deepseek-flash", Some("custom-route"))
            .unwrap()
            .source,
        "manual"
    );
    assert!(pricing::find_for_provider(&prices, "gpt-5.4", Some("custom-route")).is_none());
    assert_eq!(
        deepseek::context(timestamp("2026-10-01T10:00:00+08:00"), &settings)
            .unwrap()
            .period,
        "off_peak"
    );
    assert_eq!(
        opencode_go::context(timestamp("2026-10-01T10:00:00+08:00"))
            .unwrap()
            .period,
        "peak"
    );
    for (time, period) in [
        ("2026-10-19T01:00:00Z", "peak"),
        ("2026-10-19T04:00:00Z", "off_peak"),
        ("2026-10-19T06:00:00Z", "peak"),
        ("2026-10-19T10:00:00Z", "off_peak"),
        ("2026-10-18T01:00:00Z", "off_peak"),
    ] {
        assert_eq!(
            opencode_go::context(timestamp(time)).unwrap().period,
            period
        );
    }
}

#[test]
fn custom_deepseek_uses_official_reference_without_claiming_route_ownership() {
    let mut prices = pricing::builtin();
    let mut settings = deepseek::default_settings();
    settings
        .exchange_rates
        .insert("CNY".into(), "6.68249010".into());
    let mut parsed = usage(Some("custom"));
    parsed.model = "custom/deepseek-flash".into();
    let cost = pricing::resolve_with_settings(&parsed, None, &prices, &settings).unwrap();
    let official = pricing::estimate_at(
        &parsed.tokens,
        pricing::find_for_provider(&prices, "deepseek-flash", Some("deepseek-official")).unwrap(),
        parsed.occurred_at,
        &settings,
    )
    .unwrap();
    assert_eq!(cost.amount_usd, official.amount_usd);
    assert_eq!(cost.price_source, "deepseek_official");
    assert_eq!(cost.pricing_model.as_deref(), Some("deepseek-flash"));
    assert_eq!(cost.billing_basis.as_deref(), Some("model_reference"));
    assert_eq!(
        cost.snapshot.as_ref().unwrap().provider.as_deref(),
        Some("deepseek-official")
    );
    assert_eq!(parsed.provider.as_deref(), Some("custom"));
    assert_eq!(parsed.model, "custom/deepseek-flash");

    // An unrelated price on the custom route cannot suppress this model's fallback.
    let mut unrelated = reference();
    unrelated.model_id = "other-model".into();
    unrelated.provider = Some("custom".into());
    unrelated.source = "manual".into();
    prices.push(unrelated);
    assert_eq!(
        pricing::resolve_with_settings(&parsed, None, &prices, &settings)
            .unwrap()
            .price_source,
        "deepseek_official"
    );

    // Existing frozen estimates and native reported costs retain their authority.
    let tmp = tempfile::tempdir().unwrap();
    let e = Engine::open(&tmp.path().join("scope.db")).unwrap();
    let mut previous = record(&e, &parsed);
    previous.cost = cost.clone();
    sync::validate_record(&previous).unwrap();
    let mut changed_prices = prices.clone();
    for price in &mut changed_prices {
        if price.source == "deepseek_official" {
            price.output_cost_per_million = "99".into();
        }
    }
    assert_eq!(
        pricing::resolve_with_settings(&parsed, Some(&previous), &changed_prices, &settings)
            .unwrap(),
        cost
    );
    let mut more_tokens = parsed.clone();
    more_tokens.tokens.output *= 2;
    let updated =
        pricing::resolve_with_settings(&more_tokens, Some(&previous), &changed_prices, &settings)
            .unwrap();
    assert_eq!(updated.billing_basis.as_deref(), Some("model_reference"));
    assert_eq!(updated.snapshot, cost.snapshot);
    parsed.reported_cost = Some("0.123".into());
    assert_eq!(
        pricing::resolve_with_settings(&parsed, None, &prices, &settings)
            .unwrap()
            .amount_usd
            .as_deref(),
        Some("0.123")
    );
    parsed.reported_cost = None;

    let mut custom = reference();
    custom.provider = Some("custom".into());
    custom.source = "manual".into();
    prices.push(custom);
    let matched = pricing::resolve_with_settings(&parsed, None, &prices, &settings).unwrap();
    assert_eq!(matched.price_source, "manual");
    assert_eq!(
        matched.snapshot.unwrap().provider.as_deref(),
        Some("custom")
    );
}

#[test]
fn official_fallback_keeps_missing_exchange_rates_and_unknown_models_unknown() {
    let prices = pricing::builtin();
    let mut p = usage(Some("custom"));
    p.model = "custom/deepseek-flash".into();
    let missing_rate =
        pricing::resolve_with_settings(&p, None, &prices, &deepseek::default_settings()).unwrap();
    assert!(missing_rate.amount_usd.is_none());
    assert_eq!(missing_rate.price_source, "deepseek_official");
    assert_eq!(
        missing_rate.billing_basis.as_deref(),
        Some("model_reference")
    );
    for model in [
        "custom/deepseek-v4-flash-custom",
        "custom/private-model",
        "gpt-5.4",
    ] {
        assert!(
            pricing::find_for_provider(&prices, model, Some("custom")).is_none(),
            "{model}"
        );
    }
}

#[test]
fn repricing_repairs_unknown_custom_deepseek_costs_once_and_preserves_reported_usage() {
    let tmp = tempfile::tempdir().unwrap();
    let e = Engine::open(&tmp.path().join("scope.db")).unwrap();
    let mut p = usage(Some("custom"));
    p.model = "custom/deepseek-flash".into();
    let original = record(&e, &p);
    let mut reported = original.clone();
    reported.id = "reported".into();
    reported.cost.method = "reported".into();
    reported.cost.amount_usd = Some("0.123".into());
    {
        let s = e.lock().unwrap();
        let mut settings = deepseek::default_settings();
        settings
            .exchange_rates
            .insert("CNY".into(), "6.68249010".into());
        s.set_meta("pricing_settings", &settings).unwrap();
        s.upsert(original.clone(), "fixture").unwrap();
        s.upsert(reported.clone(), "fixture").unwrap();
    }
    assert_eq!(
        e.execute("reprice_usage", json!({"model":"deepseek-flash"}))
            .unwrap(),
        1
    );
    let updated = e.lock().unwrap().record(&p.id).unwrap().unwrap();
    assert_eq!(updated.id, original.id);
    assert_eq!(updated.model, original.model);
    assert_eq!(updated.provider, original.provider);
    assert_eq!(updated.tokens, original.tokens);
    assert_eq!(updated.occurred_at, original.occurred_at);
    assert_eq!(updated.revision, original.revision + 1);
    assert!(updated.cost.amount_usd.is_some());
    assert_eq!(updated.cost.price_source, "deepseek_official");
    assert_eq!(
        updated.cost.billing_basis.as_deref(),
        Some("model_reference")
    );
    sync::validate_record(&updated).unwrap();
    assert_eq!(
        e.lock().unwrap().record("reported").unwrap().unwrap(),
        reported
    );
    assert_eq!(
        e.execute("reprice_usage", json!({"model":"deepseek-flash"}))
            .unwrap(),
        0
    );
}

#[test]
fn parser_extraction_preserves_native_provider_per_request() {
    let mut lines: Vec<Value> = include_str!("../../../tests/fixtures/codex/session.jsonl")
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    lines[0]["payload"]["model_provider"] = json!("openai");
    lines.insert(7, json!({"type":"turn_context","payload":{"model_provider":"custom-route","model":"gpt-5.4-high"}}));
    let parsed = Codex
        .parse(Path::new("fixture.jsonl"), &lines, &TimelineIndex::new())
        .unwrap();
    assert_eq!(parsed.usage[0].provider.as_deref(), Some("openai"));
    assert_eq!(parsed.usage[1].provider.as_deref(), Some("custom-route"));
    assert_eq!(parsed.usage[1].model, "gpt-5.4-high");
    let mut events: Vec<Value> = include_str!("../../../tests/fixtures/dsh/session.jsonl")
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    let mut ordinal = 0;
    for e in &mut events {
        if e["type"] == "assistant/message" {
            e["data"]["message"]["source"]["provider"] = json!(if ordinal == 0 {
                "deepseek-official"
            } else {
                "opencode-go"
            });
            ordinal += 1;
        }
    }
    let parsed = Dsh
        .parse(Path::new("session.jsonl"), &events, &TimelineIndex::new())
        .unwrap();
    assert!(parsed
        .usage
        .iter()
        .any(|p| p.provider.as_deref() == Some("deepseek-official")));
    assert!(parsed
        .usage
        .iter()
        .any(|p| p.provider.as_deref() == Some("opencode-go")));
    assert!(harness_scope_core::providers::parse(Some("https://secret@example.com")).is_none());
}

#[test]
fn scoped_prices_coexist_and_unknowns_fill_from_models_dev() {
    let tmp = tempfile::tempdir().unwrap();
    let e = Engine::open(&tmp.path().join("scope.db")).unwrap();
    let p = usage(Some("unknown-provider"));
    let r = record(&e, &p);
    e.lock().unwrap().upsert(r, "fixture").unwrap();
    let mut scoped = reference();
    scoped.source = "manual".into();
    scoped.provider = Some("my-route".into());
    e.execute("save_prices", json!({"entries":[scoped,reference()]}))
        .unwrap();
    let r = e.lock().unwrap().local_records().unwrap().remove(0);
    assert_eq!(r.cost.amount_usd.as_deref(), Some("1.01"));
    assert_eq!(r.cost.billing_basis.as_deref(), Some("model_reference"));
    let prices: Vec<Price> =
        serde_json::from_value(e.execute("prices", json!({})).unwrap()).unwrap();
    assert_eq!(
        prices
            .iter()
            .filter(|p| p.model_id == "deepseek-v4-flash")
            .count(),
        2
    );
    e.execute(
        "delete_price",
        json!({"modelId":"deepseek-v4-flash","provider":"my-route"}),
    )
    .unwrap();
    assert!(e
        .lock()
        .unwrap()
        .prices()
        .unwrap()
        .iter()
        .any(|p| p.model_id == "deepseek-v4-flash" && p.provider.is_none()));
    assert!(e
        .execute(
            "delete_price",
            json!({"modelId":"deepseek-flash","provider":"opencode-go"})
        )
        .is_err());
    let server = Store::open(&tmp.path().join("server.db")).unwrap();
    sync::push(
        &server,
        PushRequest {
            protocol_version: 2,
            device_id: r.device_id.clone(),
            device_name: "fixture".into(),
            records: vec![r.clone()],
        },
    )
    .unwrap();
    assert_eq!(sync::changes(&server, 0, 500).unwrap().changes[0].record, r);
}

#[test]
fn historical_backfill_keeps_identity_usage_and_reported_amounts_and_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("logs");
    std::fs::create_dir(&root).unwrap();
    let text = include_str!("../../../tests/fixtures/codex/session.jsonl")
        .replace("\"cwd\":", "\"model_provider\":\"custom-route\",\"cwd\":");
    std::fs::write(root.join("session.jsonl"), text).unwrap();
    let db = tmp.path().join("scope.db");
    let e = Engine::open(&db).unwrap();
    {
        let s = e.lock().unwrap();
        let mut c = s.config().unwrap();
        c.sources = vec![Source {
            id: "fixture".into(),
            harness: "codex".into(),
            root: root.to_string_lossy().into(),
            enabled: true,
        }];
        s.set_meta("config", &c).unwrap();
    }
    e.scan(false).unwrap();
    let records = e.lock().unwrap().local_records().unwrap();
    {
        let s = e.lock().unwrap();
        for mut r in records.clone() {
            r.provider = None;
            r.cost.method = "reported".into();
            r.cost.amount_usd = Some("0.123".into());
            s.upsert(r, "fixture").unwrap();
        }
        s.conn
            .execute("UPDATE file_state SET parser_version='codex-v3'", [])
            .unwrap();
        s.conn.execute("UPDATE usage_read_cache SET payload=json_set(payload,'$.version','usage-read-v2:codex-v3')",[]).unwrap();
    }
    assert_eq!(e.scan(false).unwrap().updated, 2);
    for r in e.lock().unwrap().local_records().unwrap() {
        let old = records.iter().find(|old| old.id == r.id).unwrap();
        assert_eq!(r.provider.as_deref(), Some("custom-route"));
        assert_eq!(r.tokens, old.tokens);
        assert_eq!(r.occurred_at, old.occurred_at);
        assert_eq!(r.model, old.model);
        assert_eq!(r.cost.amount_usd.as_deref(), Some("0.123"));
    }
    let q = e
        .execute(
            "usage_query",
            json!({"kind":"logs","providerName":"custom-route"}),
        )
        .unwrap();
    assert_eq!(q["total"], 2);
    assert_eq!(
        e.execute(
            "usage_query",
            json!({"kind":"summary","providerName":"missing"})
        )
        .unwrap()["totalRequests"],
        0
    );
    drop(e);
    let e = Engine::open(&db).unwrap();
    assert_eq!(e.scan(false).unwrap().updated, 0);
    assert_eq!(e.lock().unwrap().local_records().unwrap().len(), 2);
}

#[test]
fn a_corrected_provider_replaces_a_legacy_official_estimate() {
    let tmp = tempfile::tempdir().unwrap();
    let e = Engine::open(&tmp.path().join("scope.db")).unwrap();
    let p = usage(Some("opencode-go"));
    let mut old = record(&e, &p);
    old.provider = None;
    let prices = pricing::builtin();
    let mut settings = deepseek::default_settings();
    settings
        .exchange_rates
        .insert("CNY".into(), "6.68249010".into());
    old.cost = pricing::estimate_at(
        &p.tokens,
        pricing::find_for_provider(&prices, &p.model, Some("deepseek-official")).unwrap(),
        p.occurred_at,
        &settings,
    )
    .unwrap();
    old.cost.snapshot.as_mut().unwrap().provider = None;
    let cost = pricing::resolve_with_settings(&p, Some(&old), &prices, &settings).unwrap();
    assert_eq!(cost.amount_usd.as_deref(), Some("1.506"));
    assert_eq!(cost.rule_version, opencode_go::RULE);
    assert!(cost.conversion.is_none());
}

#[test]
fn adding_official_provider_preserves_snapshots_predating_current_tariff() {
    let tmp = tempfile::tempdir().unwrap();
    let e = Engine::open(&tmp.path().join("scope.db")).unwrap();
    let mut p = usage(Some("deepseek-official"));
    p.occurred_at = timestamp("2026-08-21T10:00:00+08:00");
    let mut price = reference();
    price.source = "builtin".into();
    let mut old = record(&e, &p);
    old.provider = None;
    old.cost = pricing::estimate(&p.tokens, &price).unwrap();
    assert_eq!(
        pricing::resolve(&p, Some(&old), &pricing::builtin()).unwrap(),
        old.cost
    );
    p.provider = Some("openai".into());
    p.model = "gpt-5.4".into();
    old.model = p.model.clone();
    assert_eq!(
        pricing::resolve(&p, Some(&old), &pricing::builtin()).unwrap(),
        old.cost
    );
}
