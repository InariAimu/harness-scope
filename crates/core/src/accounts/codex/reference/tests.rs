use super::*;
use crate::model::{Cost, Tokens, UsageRecord};
const NOW: i64 = 1792479600;
fn fixture() -> (tempfile::TempDir, Engine, String) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(&dir.path().join("scope.db")).unwrap();
    let view=engine.execute("provider_account_save",json!({"provider":"openai-codex","name":"Codex","codex":{"plan":"plus"},"links":[{"harness":"codex","provider":"openai"}],"subscription":{"planName":"Manual","monthlyFee":"17","currency":"USD","periodStart":null,"renewalDate":null}})).unwrap();
    let id = view["account"]["id"].as_str().unwrap().to_owned();
    {
        let s = engine.lock().unwrap();
        let mut prices = s.prices().unwrap();
        let mut p = prices[0].clone();
        p.model_id = "gpt-reference-fixture".into();
        p.provider = Some("openai".into());
        p.source = "manual".into();
        p.currency = "USD".into();
        p.pricing_rule = None;
        p.go_tariff = None;
        p.input_cost_per_million = "1".into();
        p.output_cost_per_million = "0".into();
        p.cache_read_cost_per_million = "0.1".into();
        p.cache_creation_cost_per_million = "0".into();
        prices.push(p);
        s.set_meta("prices", &prices).unwrap();
        let mut catalog = catalog::defaults();
        let mut m = catalog.models[0].clone();
        m.id = "gpt-reference-fixture".into();
        m.name = "Fixture".into();
        m.aliases = vec![];
        m.estimates=serde_json::from_value(json!({"plus":{"weekly":{"apiEquivalentUsd":"4.8","tokens":"24000000"},"rolling":{"apiEquivalentUsd":"0.144","tokens":null}},"pro100":{"weekly":{"apiEquivalentUsd":"24","tokens":"120000000"},"rolling":null}})).unwrap();
        catalog.models.push(m);
        s.set_meta("codex_allowance_catalog_v1", &catalog).unwrap();
        let mut accounts = load(&s).unwrap();
        accounts[0].snapshot = Some(
            json!({"fetchedAt":NOW,"data":{"kind":"codexQuota","buckets":[{"id":"codex","windows":[{"id":"primary","durationMinutes":300,"usedPercent":50,"status":"ok","resetsAt":NOW+3600},{"id":"secondary","durationMinutes":10080,"usedPercent":50,"status":"ok","resetsAt":NOW+2*86400}]}]}}),
        );
        s.set_meta(META, &accounts).unwrap();
    }
    for (i, offset) in [3000, 1800, 600].into_iter().enumerate() {
        add(
            &engine,
            &format!("r{i}"),
            NOW - offset,
            "openai",
            10000,
            100000,
        );
    }
    (dir, engine, id)
}
fn add(engine: &Engine, id: &str, at: i64, provider: &str, input: u64, cache: u64) {
    let s = engine.lock().unwrap();
    let cost = Cost {
        amount_usd: Some("0".into()),
        method: "reported".into(),
        ..Default::default()
    };
    s.upsert(
        UsageRecord {
            device_id: s.device_id().unwrap(),
            id: id.into(),
            provider: Some(provider.into()),
            harness: "codex".into(),
            parser_version: "fixture".into(),
            session_id: Some("fixture".into()),
            native_event_id: Some(id.into()),
            occurred_at: at,
            model: "gpt-reference-fixture".into(),
            grain: "event".into(),
            count: 1,
            tokens: Tokens {
                input,
                cache_read: cache,
                ..Default::default()
            },
            cost,
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
fn forecast(engine: &Engine, id: &str, extra: Value) -> Value {
    let mut args = json!({"id":id,"lookbackMinutes":60});
    args.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    calculate_at(engine, &args, NOW).unwrap()
}
#[test]
fn model_specific_allowances_are_separate_from_monthly_fees_and_preserve_history() {
    let (_d, e, id) = fixture();
    let before = e.read_lock().unwrap().local_records().unwrap();
    let pending = e.read_lock().unwrap().pending().unwrap();
    let account = e.execute("provider_accounts", json!({})).unwrap();
    let plus = forecast(&e, &id, json!({}));
    assert_eq!(plus["windows"].as_array().unwrap().len(), 2);
    assert_eq!(plus["sample"]["rateUsdPerHour"], "0.06");
    assert!(
        (Decimal::from_str(plus["windows"][1]["budgetUsd"].as_str().unwrap()).unwrap()
            - Decimal::new(48, 1))
        .abs()
            < Decimal::new(1, 15)
    );
    assert_eq!(plus["windows"][0]["status"], "ready");
    assert_eq!(plus["nextEvent"]["kind"], "reset");
    for (plan, factor) in [("pro100", 5), ("pro200", 10), ("pro500", 25)] {
        let value = forecast(&e, &id, json!({"plan":plan}));
        assert_eq!(value["windows"].as_array().unwrap().len(), 1);
        assert_eq!(value["windows"][0]["id"], "weekly");
        assert_eq!(value["monthlyFeeUsd"], (20 * factor).to_string());
        let budget = Decimal::from_str(value["windows"][0]["budgetUsd"].as_str().unwrap()).unwrap();
        assert!((budget / Decimal::from(factor) - Decimal::new(48, 1)).abs() < Decimal::new(1, 15));
    }
    assert_eq!(before, e.read_lock().unwrap().local_records().unwrap());
    assert_eq!(pending, e.read_lock().unwrap().pending().unwrap());
    assert_eq!(account, e.execute("provider_accounts", json!({})).unwrap());
}
#[test]
fn reference_pace_changes_runtime_but_not_available_daily_budget_and_uses_current_prices() {
    let (_d, e, id) = fixture();
    let slow = forecast(&e, &id, json!({}));
    let fast = forecast(&e, &id, json!({"pace":"4"}));
    assert_eq!(fast["sample"]["rateUsdPerHour"], "0.24");
    assert_eq!(fast["nextEvent"]["kind"], "exhaustion");
    assert_eq!(fast["windows"][0]["runtimeSeconds"], 1080);
    assert_eq!(slow["dailyBudget"]["usd"], fast["dailyBudget"]["usd"]);
    assert_eq!(
        slow["windows"][0]["requestsPerDay"],
        fast["windows"][0]["requestsPerDay"]
    );
    {
        let s = e.lock().unwrap();
        let mut prices = s.prices().unwrap();
        let p = prices
            .iter_mut()
            .find(|p| p.model_id == "gpt-reference-fixture")
            .unwrap();
        p.input_cost_per_million = "3".into();
        s.set_meta("prices", &prices).unwrap();
    }
    assert_eq!(
        forecast(&e, &id, json!({}))["sample"]["rateUsdPerHour"],
        "0.12"
    );
    add(&e, "unlinked", NOW - 100, "other-route", 1_000_000, 0);
    assert_eq!(forecast(&e, &id, json!({}))["sample"]["requests"], 3);
}
#[test]
fn missing_stale_expired_and_zero_quota_are_distinct_from_infinite_reference_runtime() {
    let (_d, e, id) = fixture();
    assert_eq!(
        forecast(&e, &id, json!({"lookbackMinutes":15}))["sample"]["reason"],
        "insufficient_samples"
    );
    for case in ["missing", "stale", "expired", "limited", "weekly-only"] {
        {
            let s = e.lock().unwrap();
            let mut a = load(&s).unwrap();
            let snapshot = a[0].snapshot.as_mut().unwrap();
            snapshot["fetchedAt"] = json!(NOW);
            snapshot["data"]["buckets"][0]["windows"][0]["resetsAt"] = json!(NOW + 3600);
            snapshot["data"]["buckets"][0]["windows"][0]["status"] = json!("ok");
            match case {
                "missing" => {
                    snapshot["data"]["buckets"][0]["windows"][0]["durationMinutes"] = json!(60)
                }
                "stale" => snapshot["fetchedAt"] = json!(NOW - 601),
                "expired" => {
                    snapshot["data"]["buckets"][0]["windows"][0]["resetsAt"] = json!(NOW - 1)
                }
                "limited" => {
                    snapshot["data"]["buckets"][0]["windows"][0]["durationMinutes"] = json!(300);
                    snapshot["data"]["buckets"][0]["windows"][0]["status"] = json!("rate-limited");
                }
                _ => {
                    snapshot["data"]["buckets"][0]["windows"] = json!([{"id":"primary","durationMinutes":10080,"usedPercent":10,"status":"ok","resetsAt":NOW+6*86400}])
                }
            };
            s.set_meta(META, &a).unwrap();
        }
        let value = forecast(&e, &id, json!({}));
        if case == "limited" {
            assert_eq!(value["windows"][0]["runtimeSeconds"], 0);
            assert_eq!(value["windows"][0]["remainingUsd"], "0");
        } else {
            assert!(value["nextEvent"].is_null());
            assert!(value["dailyBudget"].is_null());
        }
        if case == "weekly-only" {
            assert_eq!(
                forecast(&e, &id, json!({"plan":"pro100"}))["windows"][0]["status"],
                "ready"
            );
        }
    }
}
#[test]
fn invalid_inputs_missing_prices_and_zero_rates_cannot_fabricate_a_forecast() {
    let (_d, e, id) = fixture();
    for extra in [
        json!({"pace":"0"}),
        json!({"pace":2}),
        json!({"lookbackMinutes":20}),
        json!({"plan":"pro"}),
        json!({"model":42}),
    ] {
        let mut args = json!({"id":id,"lookbackMinutes":60});
        args.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert!(calculate_at(&e, &args, NOW).is_err());
    }
    {
        let s = e.lock().unwrap();
        let mut p = s.prices().unwrap();
        let p0 = p
            .iter_mut()
            .find(|p| p.model_id == "gpt-reference-fixture")
            .unwrap();
        p0.input_cost_per_million = "0".into();
        p0.cache_read_cost_per_million = "0".into();
        s.set_meta("prices", &p).unwrap();
    }
    let zero = forecast(&e, &id, json!({}));
    assert_eq!(zero["sample"]["reason"], "zero_rate");
    assert!(zero["nextEvent"].is_null());
    {
        let s = e.lock().unwrap();
        let mut p = s.prices().unwrap();
        p.retain(|p| p.model_id != "gpt-reference-fixture");
        s.set_meta("prices", &p).unwrap();
    }
    assert_eq!(
        forecast(&e, &id, json!({}))["sample"]["reason"],
        "unpriced_samples"
    );
}

#[test]
fn unknown_capacity_and_unmapped_buckets_stay_unknown_instead_of_using_a_subscription_fee() {
    let (_d, e, id) = fixture();
    {
        let s = e.lock().unwrap();
        let mut c = catalog::load(&s).unwrap();
        c.models.retain(|m| m.id != "gpt-reference-fixture");
        s.set_meta("codex_allowance_catalog_v1", &c).unwrap();
    }
    let value = forecast(&e, &id, json!({}));
    assert_eq!(value["sample"]["rateUsdPerHour"], "0.06");
    assert_eq!(value["windows"][0]["reason"], "no_model_budget");
    assert!(value["windows"][0]["budgetUsd"].is_null());
    assert!(value["nextEvent"].is_null());
    assert!(calculate_at(&e, &json!({"id":id,"mode":"monthly-fee"}), NOW).is_err());
}

#[test]
fn current_week_clips_requests_and_the_rate_denominator_at_the_exact_reset() {
    let (_d, e, id) = fixture();
    {
        let s = e.lock().unwrap();
        let mut a = load(&s).unwrap();
        a[0].snapshot.as_mut().unwrap()["data"]["buckets"][0]["windows"][1]["resetsAt"] =
            json!(NOW - 1800 + 604800);
        s.set_meta(META, &a).unwrap();
    }
    add(&e, "before-reset", NOW - 1801, "openai", 1_000_000_000, 0);
    add(&e, "after-reset", NOW - 1200, "openai", 10000, 100000);
    for minutes in [0, 60, 1440] {
        let v = forecast(&e, &id, json!({"lookbackMinutes":minutes}));
        assert_eq!(v["sample"]["startDate"], NOW - 1800);
        assert_eq!(v["sample"]["windowStart"], NOW - 1800);
        assert_eq!(v["sample"]["durationSeconds"], 1800);
        assert_eq!(v["sample"]["requests"], 3);
        assert_eq!(v["sample"]["costUsd"], "0.06");
        assert_eq!(v["sample"]["rateUsdPerHour"], "0.12");
    }
    let recent = forecast(&e, &id, json!({"lookbackMinutes":15}));
    assert_eq!(recent["sample"]["startDate"], NOW - 900);
    assert_eq!(recent["sample"]["requests"], 1);
    assert_eq!(recent["sample"]["reason"], "insufficient_samples");
    let default = calculate_at(&e, &json!({"id":id}), NOW).unwrap();
    assert_eq!(default["lookbackMinutes"], 0);
    assert_eq!(default["sample"]["requests"], 3);
}

#[test]
fn full_window_averages_include_idle_time_and_a_just_reset_window_is_unknown() {
    let (_d, e, id) = fixture();
    {
        let s = e.lock().unwrap();
        let mut a = load(&s).unwrap();
        a[0].snapshot.as_mut().unwrap()["data"]["buckets"][0]["windows"][1]["resetsAt"] =
            json!(NOW - 2 * 86400 + 604800);
        s.set_meta(META, &a).unwrap();
    }
    let v = forecast(&e, &id, json!({"lookbackMinutes":0}));
    assert_eq!(v["sample"]["durationSeconds"], 2 * 86400);
    assert_eq!(v["sample"]["rateUsdPerHour"], "0.00125");
    {
        let s = e.lock().unwrap();
        let mut a = load(&s).unwrap();
        a[0].snapshot.as_mut().unwrap()["data"]["buckets"][0]["windows"][1]["resetsAt"] =
            json!(NOW + 604800);
        s.set_meta(META, &a).unwrap();
    }
    let v = forecast(&e, &id, json!({"lookbackMinutes":0}));
    assert_eq!(v["sample"]["requests"], 0);
    assert!(v["nextEvent"].is_null());
}
