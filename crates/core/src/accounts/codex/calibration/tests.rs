use super::*;
use crate::model::Tokens;
const NOW: i64 = 1792500000;
fn snapshot(at: i64, used: i64) -> Value {
    json!({"fetchedAt":at,"data":{"kind":"codexQuota","identity":"a".repeat(64),"identitySource":"accountId","planType":"prolite","buckets":[{"id":"codex","windows":[{"id":"primary","durationMinutes":10080,"resetsAt":NOW+7200,"usedPercent":used,"status":"ok"}]}]}})
}
fn fixture() -> (tempfile::TempDir, Engine, Value) {
    let temp = tempfile::tempdir().unwrap();
    let e = Engine::open(&temp.path().join("db.sqlite")).unwrap();
    let first = snapshot(NOW - 3600, 10);
    let last = snapshot(NOW, 20);
    record(&e.lock().unwrap(), "fixture", Some(&first), &last).unwrap();
    (temp, e, last)
}
fn row(model: &str) -> Row {
    Row {
        request_metadata: Some(crate::model::RequestMetadata {
            service_tier: Some("default".into()),
            service_tier_source: Some("turn_context".into()),
            reasoning_effort: None,
        }),
        harness: "codex".into(),
        provider: Some("openai".into()),
        model: model.into(),
        occurred_at: NOW - 100,
        count: 1,
        grain: "event".into(),
        tokens: Tokens::default(),
        amount: Some("0".into()),
        status_code: None,
        record_source: "session".into(),
        legacy_date: None,
        legacy_timezone: None,
        converted: false,
        billing_basis: None,
        timing: None,
    }
}
fn compute(e: &Engine, snapshot: &Value, rows: &[&Row], pace: i64) -> Value {
    compute_for(e, "fixture", snapshot, rows, pace)
}
fn compute_for(e: &Engine, id: &str, snapshot: &Value, rows: &[&Row], pace: i64) -> Value {
    let c = catalog::defaults();
    let input = Input {
        id,
        snapshot: Some(snapshot),
        bucket: "codex",
        catalog: &c,
        plan: "plus",
        mode: "modeldial",
        model: catalog::find(&c, "gpt-6.1-sol"),
        now: NOW,
        lookback: 86400,
        rate: Decimal::new(47, 1) / Decimal::from(3600) * Decimal::from(pace),
        request_rate: Decimal::from(3 * pace) / Decimal::from(3600),
        pace: Decimal::from(pace),
        sample_valid: true,
    };
    calculate(&e.read_lock().unwrap(), input, rows, |r| {
        match r.model.as_str() {
            "gpt-6.1-sol" => Some(Decimal::new(47, 1)),
            "gpt-6-astra" => Some(Decimal::new(66, 1)),
            _ => None,
        }
    })
    .unwrap()
}

#[test]
fn shared_observations_calibrate_after_handoff_without_the_old_local_cache() {
    use crate::accounts::{history, sharing};
    let dir = tempfile::tempdir().unwrap();
    let e = Engine::open(&dir.path().join("shared.db")).unwrap();
    let view=e.execute("provider_account_save",json!({"provider":"openai-codex","name":"Shared","sharedSubscriptionId":"new","codex":{"plan":"plus"}})).unwrap();
    let id = view["account"]["id"].as_str().unwrap();
    let shared = view["account"]["sharedSubscriptionId"].as_str().unwrap();
    let first = snapshot(NOW - 3600, 10);
    let last = snapshot(NOW, 20);
    {
        let s = e.lock().unwrap();
        let mut item = crate::accounts::load(&s).unwrap().remove(0);
        item.snapshot = Some(last.clone());
        item.request_id = Some(uuid::Uuid::new_v4().to_string());
        history::record(
            &s,
            &item,
            chrono::DateTime::from_timestamp(NOW, 0).unwrap(),
            "UTC",
        )
        .unwrap();
        let remote = sharing::Observation {
            id: uuid::Uuid::new_v4().to_string(),
            subscription_id: shared.into(),
            device_id: "previous-device".into(),
            account_id: uuid::Uuid::new_v4().to_string(),
            account_revision: "previous-revision".into(),
            account_name: "Previous".into(),
            provider: "openai-codex".into(),
            observed_at_ms: (NOW - 3600) * 1000,
            time_zone: "UTC".into(),
            data: first["data"].clone(),
        };
        sharing::apply(
            &s,
            &sharing::Changes {
                server_instance_id: "fixture".into(),
                subscriptions: sharing::list(&s).unwrap(),
                observations: vec![
                    (1, remote.clone()),
                    (
                        2,
                        sharing::Observation {
                            id: uuid::Uuid::new_v4().to_string(),
                            observed_at_ms: (NOW - 1800) * 1000,
                            data: snapshot(NOW - 1800, 5)["data"].clone(),
                            ..remote
                        },
                    ),
                ],
                next_cursor: 2,
                has_more: false,
            },
        )
        .unwrap();
        assert!(s.meta::<Value>(&key(id)).unwrap().is_none());
    }
    let sol = row("gpt-6.1-sol");
    let astra = row("gpt-6-astra");
    let result = compute_for(&e, id, &last, &[&sol, &astra], 1);
    assert_eq!(result["windows"][0]["status"], "ready", "{result}");
    close(&result["windows"][0]["calibrationFactor"], "0.5");
    assert_eq!(result["windows"][0]["observations"], 2);
    assert_eq!(result["windows"][0]["ignoredObservations"], 1);
    assert_eq!(result["historyPoints"], 3);
}
fn close(value: &Value, expected: &str) {
    let n = Decimal::from_str(value.as_str().unwrap()).unwrap();
    assert!(
        (n - Decimal::from_str(expected).unwrap()).abs() < Decimal::new(1, 12),
        "{n} vs {expected}"
    );
}
#[test]
fn mixed_model_charges_calibrate_same_account_percent_and_required_intensity() {
    let (_d, e, s) = fixture();
    let sol = row("gpt-6.1-sol");
    let astra = row("gpt-6-astra");
    let value = compute(&e, &s, &[&sol, &astra], 1);
    let w = &value["windows"][0];
    assert_eq!(w["status"], "ready");
    close(&w["observedPercentPerHour"], "10");
    close(&w["calibrationFactor"], "0.5");
    close(&w["modelPercentPerHour"], "5");
    close(&w["requiredIntensity"], "8");
    close(&w["requiredUsdPerHour"], "37.6");
    close(&w["requiredRequestsPerHour"], "24");
    assert_eq!(w["modelCount"], 2);
    assert_eq!(w["coversReset"], true);
    let fast = compute(&e, &s, &[&sol, &astra], 2);
    close(&fast["windows"][0]["modelPercentPerHour"], "10");
    close(&fast["windows"][0]["requiredIntensity"], "8");
    assert!(e.read_lock().unwrap().local_records().unwrap().is_empty());
    assert!(e.read_lock().unwrap().pending().unwrap().is_empty());
}

#[test]
fn replenishment_restarts_raw_history_and_fallback_calibration_at_the_new_remaining_quota() {
    for raw in [false, true] {
        let (_d, e, _) = fixture();
        clear(&e.lock().unwrap(), "fixture").unwrap();
        let id = if raw {
            let view = e
                .execute(
                    "provider_account_save",
                    json!({"provider":"openai-codex","name":"Adjustment","codex":{"plan":"plus"}}),
                )
                .unwrap();
            view["account"]["id"].as_str().unwrap().to_owned()
        } else {
            "fixture".into()
        };
        let mut last = Value::Null;
        for (i, (offset, used)) in [
            (-3600, 95),
            (-3000, 99),
            (-1800, 49),
            (-600, 50),
            (-300, 48),
            (0, 50),
        ]
        .into_iter()
        .enumerate()
        {
            last = snapshot(NOW + offset, used);
            last["data"]["buckets"][0]["windows"][0]["resetsAt"] =
                json!(NOW + 7200 + (i % 2) as i64);
            let store = e.lock().unwrap();
            if raw {
                let mut item = crate::accounts::load(&store).unwrap().remove(0);
                item.request_id = Some(uuid::Uuid::new_v4().to_string());
                item.snapshot = Some(last.clone());
                crate::accounts::history::record(
                    &store,
                    &item,
                    chrono::DateTime::from_timestamp(NOW + offset, 0).unwrap(),
                    "UTC",
                )
                .unwrap();
            } else {
                record(&store, &id, None, &last).unwrap();
            }
        }
        let sol = row("gpt-6.1-sol");
        let mut old_unpriced = row("unpriced-old-interval");
        old_unpriced.occurred_at = NOW - 2500;
        // The earlier unpriced usage must not block or inflate the new fit.
        let value = compute_for(&e, &id, &last, &[&old_unpriced, &sol], 1);
        let w = &value["windows"][0];
        assert_eq!(w["status"], "ready", "raw={raw}: {value}");
        assert_eq!(w["sampleStart"], NOW - 1800);
        assert_eq!(w["sampleEnd"], NOW);
        assert_eq!(w["observations"], 3);
        assert_eq!(w["ignoredObservations"], if raw { 1 } else { 0 });
        assert_eq!(w["deltaPercent"], "1");
        assert_eq!(w["linkedRequests"], 1);
        close(&w["linkedUsd"], "4.7");
        close(&w["modelPercentPerHour"], "1");
        let required = Decimal::from_str(w["requiredPercentPerHour"].as_str().unwrap()).unwrap();
        let until = Decimal::from(
            last["data"]["buckets"][0]["windows"][0]["resetsAt"]
                .as_i64()
                .unwrap()
                - NOW,
        );
        // The budget uses 50% remaining, never the old interval's 1% floor.
        assert!(
            (required * until / Decimal::from(3600) - Decimal::from(50)).abs()
                < Decimal::new(1, 10)
        );
        if raw {
            let history = e
                .execute("provider_account_history", json!({"id":id}))
                .unwrap();
            assert_eq!(history["observations"].as_array().unwrap().len(), 6);
        }
    }
}

#[test]
fn a_new_adjustment_needs_its_own_samples_before_calibration_can_resume() {
    let (_d, e, _) = fixture();
    clear(&e.lock().unwrap(), "fixture").unwrap();
    for (offset, used) in [(-3600, 95), (-1800, 99), (0, 49)] {
        record(
            &e.lock().unwrap(),
            "fixture",
            None,
            &snapshot(NOW + offset, used),
        )
        .unwrap();
    }
    let result = compute(&e, &snapshot(NOW, 49), &[&row("gpt-6.1-sol")], 1);
    assert_eq!(result["windows"][0]["observations"], 1);
    assert_eq!(result["windows"][0]["reason"], "insufficient_quota_history");
    assert!(result["windows"][0]["requiredIntensity"].is_null());
}
#[test]
fn mixed_fast_and_standard_calibration_normalizes_tiers_once_and_unknown_tiers_block_fitting() {
    let (_d, e, s) = fixture();
    let sol = row("gpt-6.1-sol");
    let mut fast = sol.clone();
    fast.request_metadata.as_mut().unwrap().service_tier = Some("priority".into());
    let data = compute(&e, &s, &[&sol, &fast], 1);
    let w = &data["windows"][0];
    // Equal API costs, 1 + 2.5 quota weights: Standard is 10 / 3.5 pp/h.
    assert_eq!(w["status"], "ready");
    close(&w["calibrationFactor"], "0.285714285714285714");
    close(&w["modelPercentPerHour"], "2.85714285714285714");
    assert_eq!(w["speedBaseline"], "standard");
    fast.request_metadata = None;
    let unknown = compute(&e, &s, &[&sol, &fast], 1);
    assert_eq!(unknown["windows"][0]["reason"], "unknown_speed_baseline");
    assert_eq!(unknown["windows"][0]["unknownSpeedRequests"], 1);
    assert!(unknown["currentWindowTarget"].is_null());
}
#[test]
fn account_switch_and_reset_stay_isolated_while_regressions_keep_valid_history() {
    let (_d, e, _) = fixture();
    let decrease = snapshot(NOW + 60, 2);
    record(&e.lock().unwrap(), "fixture", None, &decrease).unwrap();
    let h: Vec<Observation> = e
        .read_lock()
        .unwrap()
        .meta(&key("fixture"))
        .unwrap()
        .unwrap();
    assert_eq!(h.len(), 2);
    assert_eq!(h[0].used, "10");
    assert_eq!(h[1].used, "20");
    let mut changed = snapshot(NOW + 120, 3);
    changed["data"]["identity"] = json!("b".repeat(64));
    record(&e.lock().unwrap(), "fixture", Some(&decrease), &changed).unwrap();
    let h: Vec<Observation> = e
        .read_lock()
        .unwrap()
        .meta(&key("fixture"))
        .unwrap()
        .unwrap();
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].identity, "b".repeat(64));
    clear(&e.lock().unwrap(), "fixture").unwrap();
    for at in [NOW - 3600, NOW - 1800, NOW] {
        record(&e.lock().unwrap(), "fixture", None, &snapshot(at, 10)).unwrap();
    }
    let h: Vec<Observation> = e
        .read_lock()
        .unwrap()
        .meta(&key("fixture"))
        .unwrap()
        .unwrap();
    assert_eq!(h.len(), 3);
    assert_eq!(h[0].at, NOW - 3600);
    assert_eq!(h[2].at, NOW);
    let flat = compute(&e, &snapshot(NOW, 10), &[], 1);
    assert_eq!(flat["windows"][0]["reason"], "no_quota_growth");
    let mut reset = snapshot(NOW, 15);
    reset["data"]["buckets"][0]["windows"][0]["resetsAt"] = json!(NOW + 604800);
    assert_eq!(
        compute(&e, &reset, &[], 1)["windows"][0]["reason"],
        "insufficient_quota_history"
    );
}
#[test]
fn missing_local_prices_or_identity_do_not_assign_all_quota_to_the_selected_model() {
    let (_d, e, s) = fixture();
    let empty = compute(&e, &s, &[], 1);
    assert_eq!(empty["windows"][0]["reason"], "no_linked_calibration_usage");
    assert!(empty["windows"][0]["observedPercentPerHour"].is_string());
    let missing = row("unpriced-model");
    let value = compute(&e, &s, &[&missing], 1);
    assert_eq!(value["windows"][0]["reason"], "unpriced_calibration");
    assert!(value["windows"][0]["requiredIntensity"].is_null());
    let mut unverified = s.clone();
    unverified["data"]["identitySource"] = json!("email");
    assert_eq!(compute(&e, &unverified, &[], 1)["identityVerified"], false);
    assert_eq!(compute(&e, &unverified, &[], 1)["windows"], json!([]));
    let mut stale = s.clone();
    stale["fetchedAt"] = json!(NOW - 601);
    assert_eq!(
        compute(&e, &stale, &[], 1)["windows"][0]["reason"],
        "stale_quota"
    );
}

#[test]
fn even_matching_reset_groups_cannot_calibrate_with_snapshots_before_window_start() {
    let (_d, e, _) = fixture();
    clear(&e.lock().unwrap(), "fixture").unwrap();
    let mut first = snapshot(NOW - 3600, 10);
    let mut last = snapshot(NOW, 20);
    for s in [&mut first, &mut last] {
        s["data"]["buckets"][0]["windows"][0]["resetsAt"] = json!(NOW - 1800 + 604800);
    }
    record(&e.lock().unwrap(), "fixture", Some(&first), &last).unwrap();
    let v = compute(&e, &last, &[], 1);
    assert_eq!(v["windows"][0]["observations"], 1);
    assert_eq!(v["windows"][0]["reason"], "insufficient_quota_history");
}
