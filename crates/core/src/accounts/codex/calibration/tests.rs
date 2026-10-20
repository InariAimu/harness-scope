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
    let c = catalog::defaults();
    let input = Input {
        id: "fixture",
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
fn account_switch_reset_and_decrease_never_mix_observations_and_plateaus_keep_idle_time() {
    let (_d, e, _) = fixture();
    let decrease = snapshot(NOW + 60, 2);
    record(&e.lock().unwrap(), "fixture", None, &decrease).unwrap();
    let h: Vec<Observation> = e
        .read_lock()
        .unwrap()
        .meta(&key("fixture"))
        .unwrap()
        .unwrap();
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].used, "2");
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
    assert_eq!(empty["windows"][0]["reason"], "no_local_calibration_usage");
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
