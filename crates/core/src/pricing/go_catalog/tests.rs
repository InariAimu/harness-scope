use super::*;
fn at(value: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .unwrap()
        .timestamp()
}
#[test]
fn official_catalog_has_distinct_model_limits_and_explicit_peak_prices() {
    let c = defaults();
    validate(&c).unwrap();
    assert_eq!(c.models.len(), 29);
    let m = find(&c, "opencode/deepseek-v4.1-flash-high").unwrap();
    let tokens = Tokens {
        input: 1_000_000,
        output: 1_000_000,
        cache_read: 1_000_000,
        ..Default::default()
    };
    for (time, cost, period) in [
        ("2026-10-19T00:59:59Z", "0.753", "off_peak"),
        ("2026-10-19T01:00:00Z", "1.506", "peak"),
        ("2026-10-19T04:00:00Z", "0.753", "off_peak"),
        ("2026-10-19T06:00:00Z", "1.506", "peak"),
        ("2026-10-19T10:00:00Z", "0.753", "off_peak"),
        ("2026-10-18T01:00:00Z", "0.753", "off_peak"),
        ("2026-10-01T02:00:00Z", "1.506", "peak"),
    ] {
        let (v, context, _) = quote(m, &c.peak_schedule, &tokens, at(time), None).unwrap();
        assert_eq!(v.to_string(), cost);
        assert_eq!(context.unwrap().period, period);
    }
    assert_eq!(m.tiers[0].monthly_limits.go.as_deref(), Some("80"));
    assert_eq!(m.tiers[0].monthly_limits.go_plus.as_deref(), Some("160"));
    let pro = find(&c, "deepseek-v4-pro").unwrap();
    assert_eq!(
        quote(
            pro,
            &c.peak_schedule,
            &tokens,
            at("2026-10-19T01:00:00Z"),
            None
        )
        .unwrap()
        .0
        .to_string(),
        "5.324"
    );
}

#[test]
fn tier_selection_counts_prompt_cache_but_not_output_and_does_not_guess_write_prices() {
    let catalog = defaults();
    let model = find(&catalog, "qwen3.7-plus").unwrap();
    for (input, cache, output, expected) in [
        (256000, 0, 1000000, "0.4"),
        (0, 256000, 1000000, "0.4"),
        (1, 256000, 0, "1.2"),
    ] {
        let tokens = Tokens {
            input,
            cache_read: cache,
            output,
            ..Default::default()
        };
        assert_eq!(
            quote(
                model,
                &catalog.peak_schedule,
                &tokens,
                at("2026-10-20T10:00:00Z"),
                None
            )
            .unwrap()
            .2
            .input,
            expected
        );
    }
    let model = find(&catalog, "deepseek-flash").unwrap();
    assert!(quote(
        model,
        &catalog.peak_schedule,
        &Tokens {
            input: 1000,
            cache_write: 10,
            ..Default::default()
        },
        at("2026-10-20T10:00:00Z"),
        None
    )
    .is_err());
    let model = find(&catalog, "claude-haiku-5-5").unwrap();
    assert!(quote(
        model,
        &catalog.peak_schedule,
        &Tokens {
            input: 1000,
            cache_write: 10,
            cache_write1h: 10,
            ..Default::default()
        },
        at("2026-10-20T10:00:00Z"),
        None
    )
    .is_err());
    let free = find(&catalog, "beam-free").unwrap();
    assert_eq!(
        quote(
            free,
            &catalog.peak_schedule,
            &Tokens {
                input: 1000,
                cache_write: 10,
                cache_write1h: 10,
                ..Default::default()
            },
            at("2026-10-20T10:00:00Z"),
            None
        )
        .unwrap()
        .0,
        Decimal::ZERO
    );
}

#[test]
fn invalid_rules_and_ambiguous_aliases_are_rejected() {
    let base = serde_json::to_value(defaults()).unwrap();
    for patch in [
        ("/models/0/tiers/0/prices/input", json!("-1")),
        ("/models/0/tiers/0/monthlyLimits/go", json!("0")),
        ("/models/0/tiers/0/monthlyLimits/go", Value::Null),
        ("/models/0/aliases", json!(["glm-5.3"])),
        ("/peakSchedule/timezone", json!("Asia/Shanghai")),
        ("/peakSchedule/intervals", json!([[240, 60]])),
        ("/peakSchedule/weekdays", json!([1, 1])),
        ("/windowFractions/monthly", json!("0")),
        ("/schemaVersion", json!(2)),
    ] {
        let mut value = base.clone();
        *value.pointer_mut(patch.0).unwrap() = patch.1;
        let catalog: Catalog = serde_json::from_value(value).unwrap();
        assert!(validate(&catalog).is_err(), "{}", patch.0);
    }
    assert!(allowance_percent(Decimal::MAX, Some(Decimal::new(1, 6)), Decimal::new(1, 6)).is_err());
}

#[test]
fn files_are_staged_validated_and_applied_with_conflict_and_failure_rollback() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("scope.db");
    let engine = crate::Engine::open(&db).unwrap();
    let path = db.with_extension("opencode-go.json");
    let initial = engine.execute("go_catalog", json!({})).unwrap();
    assert!(!path.exists());
    fs::write(&path, b"not JSON; do not execute or echo sensitive values").unwrap();
    let invalid = engine.execute("go_catalog", json!({})).unwrap();
    assert!(invalid["file"]["error"].is_string());
    assert_eq!(invalid["catalog"], initial["catalog"]);
    let mut catalog = defaults();
    catalog.version = "updated-test".into();
    catalog
        .models
        .iter_mut()
        .find(|m| m.id == "deepseek-flash")
        .unwrap()
        .tiers[0]
        .peak_prices
        .as_mut()
        .unwrap()
        .input = "0.99".into();
    let saved=engine.execute("save_go_catalog",json!({"catalog":catalog,"expectedRevision":invalid["revision"],"expectedFileRevision":invalid["file"]["revision"]})).unwrap();
    assert_eq!(saved["catalog"]["version"], "updated-test");
    let bytes = fs::read(&path).unwrap();
    assert!(engine.execute("save_go_catalog",json!({"catalog":defaults(),"expectedRevision":initial["revision"],"expectedFileRevision":saved["file"]["revision"]})).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    fs::write(&path, serde_json::to_vec(&defaults()).unwrap()).unwrap();
    assert!(engine.execute("save_go_catalog",json!({"catalog":catalog,"expectedRevision":saved["revision"],"expectedFileRevision":saved["file"]["revision"]})).is_err());
    let staged = engine.execute("go_catalog", json!({})).unwrap();
    assert_eq!(staged["catalog"]["version"], "updated-test");
    assert_eq!(staged["file"]["catalog"]["version"], "2026-10-20");
    engine.lock().unwrap().conn.execute_batch("CREATE TRIGGER reject_go BEFORE UPDATE ON meta WHEN NEW.key='opencode_go_catalog_v1' BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
    let old_file = fs::read(&path).unwrap();
    assert!(engine.execute("save_go_catalog",json!({"catalog":defaults(),"expectedRevision":staged["revision"],"expectedFileRevision":staged["file"]["revision"]})).is_err());
    assert_eq!(fs::read(&path).unwrap(), old_file);
    assert_eq!(
        engine.execute("go_catalog", json!({})).unwrap()["catalog"],
        saved["catalog"]
    );
    fs::write(&path, vec![b' '; MAX_BYTES + 1]).unwrap();
    let oversized = engine.execute("go_catalog", json!({})).unwrap();
    assert!(oversized["file"]["error"].is_string());
    assert_eq!(oversized["catalog"], saved["catalog"]);
}

#[test]
fn configured_peak_rates_freeze_in_cost_snapshots_and_survive_sync() {
    use crate::model::{ParsedUsage, PushRequest, UsageRecord};
    let tmp = tempfile::tempdir().unwrap();
    let engine = crate::Engine::open(&tmp.path().join("scope.db")).unwrap();
    let initial = engine.execute("go_catalog", json!({})).unwrap();
    let mut c = defaults();
    let flash = c
        .models
        .iter_mut()
        .find(|m| m.id == "deepseek-flash")
        .unwrap();
    flash.tiers[0].peak_prices.as_mut().unwrap().input = "0.99".into();
    engine
        .execute(
            "save_go_catalog",
            json!({"catalog":c,"expectedRevision":initial["revision"],"expectedFileRevision":null}),
        )
        .unwrap();
    let parsed = ParsedUsage {
        request_metadata: None,
        provider: Some("opencode-go".into()),
        id: "p".into(),
        session_id: "s".into(),
        native_event_id: "p".into(),
        occurred_at: at("2026-10-20T01:00:00Z"),
        model: "opencode/deepseek-v4.1-flash-high".into(),
        tokens: Tokens {
            input: 1_000_000,
            ..Default::default()
        },
        reported_cost: None,
        timing: None,
    };
    let prices = engine.lock().unwrap().prices().unwrap();
    let settings = crate::pricing::deepseek::default_settings();
    let cost = crate::pricing::resolve_with_settings(&parsed, None, &prices, &settings).unwrap();
    assert_eq!(cost.amount_usd.as_deref(), Some("0.99"));
    assert_eq!(
        cost.snapshot.as_ref().unwrap().input_cost_per_million,
        "0.99"
    );
    assert_eq!(cost.context.as_ref().unwrap().period, "peak");
    let record = UsageRecord {
        timing: None,
        request_metadata: None,
        provider: parsed.provider.clone(),
        device_id: engine.lock().unwrap().device_id().unwrap(),
        id: "p".into(),
        harness: "dsh".into(),
        parser_version: "fixture".into(),
        session_id: Some("s".into()),
        native_event_id: Some("p".into()),
        occurred_at: parsed.occurred_at,
        model: parsed.model.clone(),
        grain: "event".into(),
        count: 1,
        tokens: parsed.tokens.clone(),
        cost,
        record_source: "session".into(),
        revision: 1,
        deleted: false,
        status_code: None,
        legacy_date: None,
        legacy_timezone: None,
    };
    engine
        .lock()
        .unwrap()
        .upsert(record.clone(), "fixture")
        .unwrap();
    let version = engine.lock().unwrap().usage_version().unwrap();
    let before = engine.lock().unwrap().pending().unwrap();
    let info = engine.execute("go_catalog", json!({})).unwrap();
    c.models
        .iter_mut()
        .find(|m| m.id == "deepseek-flash")
        .unwrap()
        .tiers[0]
        .peak_prices
        .as_mut()
        .unwrap()
        .input = "0.42".into();
    engine.execute("save_go_catalog",json!({"catalog":c,"expectedRevision":info["revision"],"expectedFileRevision":info["file"]["revision"]})).unwrap();
    assert_eq!(engine.lock().unwrap().usage_version().unwrap(), version);
    assert_eq!(engine.lock().unwrap().pending().unwrap(), before);
    let prices = engine.lock().unwrap().prices().unwrap();
    assert_eq!(
        crate::pricing::resolve_with_settings(&parsed, Some(&record), &prices, &settings).unwrap(),
        record.cost
    );
    let mut amended = parsed.clone();
    amended.tokens.input = 2_000_000;
    assert_eq!(
        crate::pricing::resolve_with_settings(&amended, Some(&record), &prices, &settings)
            .unwrap()
            .amount_usd
            .as_deref(),
        Some("1.98")
    );
    assert_eq!(
        crate::pricing::resolve_with_settings(&parsed, None, &prices, &settings)
            .unwrap()
            .amount_usd
            .as_deref(),
        Some("0.42")
    );
    let server = Store::open(&tmp.path().join("server.db")).unwrap();
    crate::sync::push(
        &server,
        PushRequest {
            protocol_version: 2,
            device_id: record.device_id.clone(),
            device_name: "fixture".into(),
            records: vec![record.clone()],
        },
    )
    .unwrap();
    assert_eq!(
        crate::sync::changes(&server, 0, 10).unwrap().changes[0].record,
        record
    );
    let mut reported = parsed;
    reported.reported_cost = Some("0.123".into());
    assert_eq!(
        crate::pricing::resolve_with_settings(&reported, None, &prices, &settings)
            .unwrap()
            .amount_usd
            .as_deref(),
        Some("0.123")
    );
}
