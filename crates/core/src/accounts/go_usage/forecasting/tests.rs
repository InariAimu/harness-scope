use super::*;
use crate::model::{Cost, UsageRecord};

const NOW: i64 = 1792451400; // Monday, 2026-10-19 23:10 UTC (off-peak).
fn fixture() -> (tempfile::TempDir, Engine, String) {
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::open(&tmp.path().join("scope.db")).unwrap();
    let account=engine.execute("provider_account_save",json!({"provider":"opencode-go","name":"Go","links":[{"harness":"dsh","provider":"opencode-go"}],"subscription":{"planName":"Manual","monthlyFee":"7","currency":"USD","periodStart":null,"renewalDate":null}})).unwrap();
    let id = account["account"]["id"].as_str().unwrap().to_string();
    {
        let store = engine.lock().unwrap();
        let mut catalog = go_catalog::defaults();
        let m = catalog
            .models
            .iter_mut()
            .find(|m| m.id == "deepseek-flash")
            .unwrap();
        for t in &mut m.tiers {
            t.prices.input = "1".into();
            t.prices.output = "0".into();
            t.prices.cache_read = "0".into();
            t.peak_prices = Some(go_catalog::Rates {
                input: "2".into(),
                ..t.prices.clone()
            });
            t.monthly_limits.go = Some("100".into());
            t.monthly_limits.go_plus = Some("200".into());
        }
        go_catalog::validate(&catalog).unwrap();
        store.set_meta("opencode_go_catalog_v1", &catalog).unwrap();
        let mut accounts = load(&store).unwrap();
        accounts[0].snapshot = Some(
            json!({"fetchedAt":NOW,"data":{"kind":"quota","windows":[{"id":"rolling","usedPercent":50,"status":"ok","resetsAt":NOW+3600},{"id":"weekly","usedPercent":50,"status":"ok","resetsAt":NOW+2*86400},{"id":"monthly","usedPercent":50,"status":"ok","resetsAt":NOW+10*86400}]}}),
        );
        store.set_meta(META, &accounts).unwrap();
    }
    for (i, at) in [NOW - 3000, NOW - 1800, NOW - 600].into_iter().enumerate() {
        add(
            &engine,
            &format!("r{i}"),
            "deepseek-v4.1-flash",
            at,
            1_000_000,
        );
    }
    (tmp, engine, id)
}
fn add(engine: &Engine, id: &str, model: &str, time: i64, input: u64) {
    let store = engine.lock().unwrap();
    store
        .upsert(
            UsageRecord {
                timing: None,
                request_metadata: None,
                device_id: store.device_id().unwrap(),
                id: id.into(),
                provider: Some("opencode-go".into()),
                harness: "dsh".into(),
                parser_version: "fixture".into(),
                session_id: Some("fixture".into()),
                native_event_id: Some(id.into()),
                occurred_at: time,
                model: model.into(),
                grain: "event".into(),
                count: 1,
                tokens: Tokens {
                    input,
                    ..Default::default()
                },
                cost: Cost::default(),
                record_source: "session".into(),
                revision: 1,
                deleted: false,
                status_code: None,
                legacy_date: None,
                legacy_timezone: None,
            },
            "fixture",
        )
        .unwrap();
}
fn predict(engine: &Engine, id: &str, extra: Value) -> Value {
    let mut args = json!({"id":id,"plan":"go","lookbackMinutes":60});
    args.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    calculate_at(engine, &args, NOW).unwrap()
}

#[test]
fn shared_subscription_forecasts_include_remote_samples_and_post_snapshot_usage_once() {
    let (_tmp, engine, id) = fixture();
    {
        let s = engine.lock().unwrap();
        let mut remote = s.record("r1").unwrap().unwrap();
        // A fixture's local ID and payload move to the remote cache, as on another terminal.
        s.conn
            .execute("DELETE FROM records WHERE id='r1'", [])
            .unwrap();
        remote.device_id = "other-device".into();
        crate::accounts::tests::cache_remote(&s, &remote);
        let own = s.record("r0").unwrap().unwrap();
        crate::accounts::tests::cache_remote(&s, &own);
        let mut extra = remote.clone();
        extra.id = "deleted".into();
        extra.deleted = true;
        crate::accounts::tests::cache_remote(&s, &extra);
        extra.deleted = false;
        extra.id = "unlinked".into();
        extra.provider = Some("other-provider".into());
        crate::accounts::tests::cache_remote(&s, &extra);
    }
    let base = predict(&engine, &id, json!({}));
    assert_eq!(base["sample"]["requests"], 3);
    assert_eq!(base["sample"]["currentUsdPerHour"], "3");
    assert_eq!(base["windows"][0]["remainingPercent"], "50");
    {
        let s = engine.lock().unwrap();
        let mut accounts = load(&s).unwrap();
        accounts[0].snapshot.as_mut().unwrap()["fetchedAt"] = json!(NOW - 100);
        s.set_meta(META, &accounts).unwrap();
        let mut remote = s.record("r0").unwrap().unwrap();
        remote.device_id = "other-device".into();
        remote.id = "after-snapshot".into();
        remote.occurred_at = NOW - 50;
        crate::accounts::tests::cache_remote(&s, &remote);
    }
    let after = predict(&engine, &id, json!({}));
    assert_eq!(after["sample"]["requests"], 4);
    assert_eq!(after["sample"]["currentUsdPerHour"], "4");
    assert_eq!(after["windows"][0]["postSnapshotPercent"], "5");
    assert_eq!(after["windows"][0]["remainingPercent"], "45");
    assert_eq!(after["windows"][0]["remainingModelUsd"], "9");
    {
        let s = engine.lock().unwrap();
        let mut unknown = s.record("r0").unwrap().unwrap();
        unknown.device_id = "other-device".into();
        unknown.id = "unknown-remote".into();
        unknown.model = "unknown-model".into();
        unknown.occurred_at = NOW - 25;
        crate::accounts::tests::cache_remote(&s, &unknown);
    }
    let unknown = predict(&engine, &id, json!({"model":"deepseek-flash"}));
    assert_eq!(unknown["windows"][0]["reason"], "unpriced_since_snapshot");
}

#[test]
fn remote_cache_write_durations_use_the_remote_payload_for_pricing() {
    let (_tmp, engine, id) = fixture();
    let s = engine.lock().unwrap();
    let mut remote = s.record("r0").unwrap().unwrap();
    s.conn.execute("DELETE FROM records", []).unwrap();
    remote.device_id = "other-device".into();
    remote.tokens.input = 0;
    remote.tokens.cache_write = 1_000_000;
    remote.tokens.cache_write1h = 750_000;
    crate::accounts::tests::cache_remote(&s, &remote);
    let mut catalog = go_catalog::load(&s).unwrap();
    let m = catalog
        .models
        .iter_mut()
        .find(|m| m.id == "deepseek-flash")
        .unwrap();
    for tier in &mut m.tiers {
        tier.prices.cache_write = Some("2".into());
        tier.prices.cache_write1h = Some("4".into());
    }
    let account = load(&s)
        .unwrap()
        .into_iter()
        .find(|a| a.account.id == id)
        .unwrap()
        .account;
    let facts = facts(&s, &account, NOW - 3600, NOW).unwrap();
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].tokens.cache_write1h, 750_000);
    let model = go_catalog::find(&catalog, "deepseek-flash").unwrap();
    assert_eq!(
        priced(model, &catalog, &facts[0], Some(false), "go")
            .unwrap()
            .usd,
        Decimal::new(35, 1)
    );
}
#[test]
fn forecasts_use_remaining_quota_rate_and_distinct_windows_without_writing_history() {
    let (_tmp, engine, id) = fixture();
    let (before, pending, accounts) = {
        let s = engine.read_lock().unwrap();
        (
            s.local_records().unwrap(),
            s.pending().unwrap(),
            s.meta::<Value>(META).unwrap(),
        )
    };
    let value = predict(&engine, &id, json!({}));
    assert_eq!(value["model"], "deepseek-flash");
    assert_eq!(value["sample"]["requests"], 3);
    assert_eq!(value["sample"]["currentUsdPerHour"], "3");
    let rolling = &value["windows"][0];
    assert_eq!(rolling["status"], "ready");
    assert_eq!(rolling["remainingPercent"], "50");
    assert_eq!(rolling["remainingModelUsd"], "10");
    assert_eq!(rolling["requestBudget"], "10");
    assert_eq!(rolling["exhaustsBeforeReset"], false);
    assert_eq!(rolling["runtimeSeconds"], 3600);
    assert_eq!(value["nextEvent"]["kind"], "reset");
    assert_eq!(value["windows"][1]["remainingModelUsd"], "25");
    assert!(value["windows"][1]["exhaustsBeforeReset"]
        .as_bool()
        .unwrap());
    let plus = predict(&engine, &id, json!({"plan":"goPlus"}));
    assert_eq!(plus["windows"][0]["remainingModelUsd"], "20");
    let fast = predict(&engine, &id, json!({"pace":"4"}));
    assert_eq!(fast["nextEvent"]["kind"], "exhaustion");
    assert_eq!(fast["windows"][0]["exhaustionAt"], NOW + 3000);
    assert_eq!(
        value["windows"][0]["requestsPerDay"],
        fast["windows"][0]["requestsPerDay"]
    );
    let s = engine.read_lock().unwrap();
    assert_eq!(before, s.local_records().unwrap());
    assert_eq!(pending, s.pending().unwrap());
    assert_eq!(accounts, s.meta::<Value>(META).unwrap());
}
#[test]
fn future_peak_transitions_and_weekends_are_integrated_instead_of_fixed_current_rates() {
    let catalog = go_catalog::defaults();
    let start = chrono::DateTime::parse_from_rfc3339("2026-10-20T00:30:00Z")
        .unwrap()
        .timestamp();
    let intervals = spans(&catalog.peak_schedule, start, start + 7200).unwrap();
    let rates = [
        Rate {
            normalized: Decimal::ONE / Decimal::from(3600),
            usd: Decimal::ONE / Decimal::from(3600),
        },
        Rate {
            normalized: Decimal::from(2) / Decimal::from(3600),
            usd: Decimal::from(2) / Decimal::from(3600),
        },
    ];
    let (cost, exhausted) = integrate(&intervals, rates, Decimal::from(2)).unwrap();
    assert!((cost.usd - Decimal::new(35, 1)).abs() < Decimal::new(1, 20));
    assert!((exhausted.unwrap() - (start + 4500)).abs() <= 1);
    let saturday = chrono::DateTime::parse_from_rfc3339("2026-10-24T02:00:00Z")
        .unwrap()
        .timestamp();
    assert!(spans(&catalog.peak_schedule, saturday, saturday + 86400)
        .unwrap()
        .iter()
        .all(|s| !s.peak));
    assert!(spans(&catalog.peak_schedule, start, start + MAX_HORIZON + 1).is_err());
}
#[test]
fn stale_missing_or_expired_quota_and_unknown_post_snapshot_usage_do_not_imply_free_capacity() {
    let (_tmp, engine, id) = fixture();
    let change = |field: &str, value: Value| {
        let s = engine.lock().unwrap();
        let mut items = load(&s).unwrap();
        items[0].snapshot.as_mut().unwrap()[field] = value;
        s.set_meta(META, &items).unwrap();
    };
    change("fetchedAt", json!(NOW - 601));
    let stale = predict(&engine, &id, json!({}));
    assert!(stale["nextEvent"].is_null());
    assert_eq!(stale["windows"][0]["reason"], "stale_quota");
    change("fetchedAt", json!(NOW - 100));
    add(&engine, "unknown", "unknown-model", NOW - 50, 1000);
    let unknown = predict(&engine, &id, json!({"model":"deepseek-flash"}));
    assert_eq!(unknown["windows"][0]["reason"], "unpriced_since_snapshot");
    {
        let s = engine.lock().unwrap();
        s.conn
            .execute("delete from records where id='unknown'", [])
            .unwrap();
        let mut items = load(&s).unwrap();
        let snapshot = items[0].snapshot.as_mut().unwrap();
        snapshot["data"]["windows"][0]["resetsAt"] = json!(NOW);
        s.set_meta(META, &items).unwrap();
    }
    let expired = predict(&engine, &id, json!({}));
    assert_eq!(expired["windows"][0]["reason"], "expired_window");
    assert!(expired["dailyBudget"].is_null());
    {
        let s = engine.lock().unwrap();
        let mut items = load(&s).unwrap();
        items[0].snapshot = None;
        s.set_meta(META, &items).unwrap();
    }
    assert_eq!(
        predict(&engine, &id, json!({}))["windows"][0]["reason"],
        "no_quota"
    );
}
#[test]
fn snapshots_are_brought_forward_across_models_and_limited_windows_have_zero_budget() {
    let (_tmp, engine, id) = fixture();
    {
        let s = engine.lock().unwrap();
        let mut items = load(&s).unwrap();
        items[0].snapshot.as_mut().unwrap()["fetchedAt"] = json!(NOW - 100);
        s.set_meta(META, &items).unwrap();
    }
    add(&engine, "after", "deepseek-v4.1-flash", NOW - 50, 1_000_000);
    let result = predict(&engine, &id, json!({}));
    assert_eq!(result["windows"][0]["remainingPercent"], "45");
    assert_eq!(result["windows"][0]["postSnapshotPercent"], "5");
    assert_eq!(result["windows"][0]["remainingModelUsd"], "9");
    add(
        &engine,
        "other-model",
        "deepseek-v4-pro",
        NOW - 25,
        1_000_000,
    );
    let mixed = predict(&engine, &id, json!({"model":"deepseek-flash"}));
    assert_eq!(mixed["windows"][0]["remainingPercent"], "23");
    assert_eq!(mixed["windows"][0]["remainingModelUsd"], "4.6");
    {
        let s = engine.lock().unwrap();
        let mut items = load(&s).unwrap();
        items[0].snapshot.as_mut().unwrap()["data"]["windows"][0]["status"] = json!("rate-limited");
        s.set_meta(META, &items).unwrap();
    }
    let limited = predict(&engine, &id, json!({"model":"deepseek-flash"}));
    assert_eq!(limited["windows"][0]["remainingPercent"], "0");
    assert_eq!(limited["windows"][0]["runtimeSeconds"], 0);
    assert_eq!(limited["windows"][0]["requestBudget"], "0");
}
#[test]
fn request_mix_keeps_context_tiers_and_insufficient_or_free_models_stay_explicit() {
    let (_tmp, engine, id) = fixture();
    let missing = predict(&engine, &id, json!({"model":"deepseek-v4-pro"}));
    assert_eq!(missing["sample"]["reason"], "insufficient_samples");
    assert!(missing["nextEvent"].is_null());
    assert_eq!(
        predict(&engine, &id, json!({"model":"beam-free"}))["sample"]["reason"],
        "free_model"
    );
    assert_eq!(
        predict(&engine, &id, json!({"lookbackMinutes":15}))["sample"]["reason"],
        "insufficient_samples"
    );
    let s = engine.read_lock().unwrap();
    let catalog = go_catalog::load(&s).unwrap();
    let model = go_catalog::find(&catalog, "qwen3.7-plus").unwrap();
    let small = Fact {
        model: model.id.clone(),
        time: NOW,
        count: 1,
        grain: "event".into(),
        tokens: Tokens {
            input: 100_000,
            ..Default::default()
        },
    };
    let a = priced(model, &catalog, &small, Some(false), "go").unwrap();
    let large = Fact {
        tokens: Tokens {
            input: 300_000,
            ..Default::default()
        },
        ..small
    };
    let b = priced(model, &catalog, &large, Some(false), "go").unwrap();
    assert_eq!(a.usd, Decimal::new(4, 2));
    assert_eq!(b.usd, Decimal::new(36, 2));
    drop(s);
    for (id, time, input) in [
        ("q1", NOW - 1500, 100_000),
        ("q2", NOW - 1200, 100_000),
        ("q3", NOW - 900, 300_000),
    ] {
        add(&engine, id, "qwen3.7-plus", time, input);
    }
    let mix = predict(&engine, &id, json!({"model":"qwen3.7-plus"}));
    assert_eq!(mix["sample"]["currentUsdPerHour"], "0.44");
    assert_eq!(mix["windows"][0]["requestBudget"], "40");
}

#[test]
fn unavailable_cache_rates_and_invalid_forecast_controls_are_rejected_without_mutation() {
    let (_tmp, engine, id) = fixture();
    {
        let s = engine.lock().unwrap();
        let mut record = s.record("r0").unwrap().unwrap();
        record.tokens.cache_write = 100;
        s.upsert(record, "fixture").unwrap();
    }
    let unpriced = predict(&engine, &id, json!({"model":"deepseek-flash"}));
    assert_eq!(unpriced["sample"]["reason"], "unpriced_samples");
    assert!(unpriced["nextEvent"].is_null());
    for bad in [
        json!({"lookbackMinutes":1}),
        json!({"lookbackMinutes":"60"}),
        json!({"pace":"0"}),
        json!({"pace":"11"}),
        json!({"pace":2}),
        json!({"model":5}),
    ] {
        let mut args = json!({"id":id,"plan":"go"});
        args.as_object_mut()
            .unwrap()
            .extend(bad.as_object().unwrap().clone());
        assert!(calculate_at(&engine, &args, NOW).is_err());
    }
}
