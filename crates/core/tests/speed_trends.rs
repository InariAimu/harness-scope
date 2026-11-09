use harness_scope_core::{model::*, query, store::Store};
use serde_json::{json, Value};

const AT: i64 = 1_800_000_000; // UTC bucket boundary
fn record(s: &Store, id: &str, at: i64, output: u64) -> UsageRecord {
    UsageRecord {
        model_usage: None,
        timing: None,
        request_metadata: None,
        device_id: s.device_id().unwrap(),
        id: id.into(),
        revision: 1,
        harness: "codex".into(),
        provider: Some("route".into()),
        model: "model".into(),
        parser_version: "fixture".into(),
        session_id: None,
        native_event_id: None,
        occurred_at: at,
        grain: "event".into(),
        count: 1,
        tokens: Tokens {
            output,
            ..Default::default()
        },
        cost: Cost::default(),
        record_source: "session".into(),
        deleted: false,
        status_code: None,
        legacy_date: None,
        legacy_timezone: None,
    }
}
fn add(s: &Store, id: &str, at: i64, output: u64, duration: i64, first: Option<i64>) {
    s.upsert(record(s, id, at, output), "fixture").unwrap();
    if duration > 0 {
        s.save_timing(
            id,
            Some(&UsageTiming {
                duration_ms: duration,
                first_token_ms: first,
                duration_source: "fixture".into(),
                first_token_source: first.map(|_| "fixture".into()),
            }),
        )
        .unwrap();
    }
}
fn read(s: &Store, args: Value) -> Value {
    let mut defaults = json!({"startDate":AT,"endDate":AT+1199});
    defaults
        .as_object_mut()
        .unwrap()
        .extend(args.as_object().unwrap().clone());
    query::query(s, "speedTrends", &defaults).unwrap()
}

#[test]
fn strict_decode_population_trimmed_observed_bounds_and_weighted_mean_are_read_only() {
    let tmp = tempfile::tempdir().unwrap();
    let s = Store::open(&tmp.path().join("scope.db")).unwrap();
    for (i, speed) in [10, 20, 30, 40, 50, 60, 70, 800].into_iter().enumerate() {
        let seconds = if i == 7 { 40 } else { 20 };
        add(
            &s,
            &format!("valid-{i}"),
            AT + 10,
            speed * seconds,
            (seconds * 1000 + 1000) as i64,
            Some(1000),
        );
    }
    for (id, output, duration, first) in [
        ("e2e", 900, 10000, None),
        ("short", 199, 5000, Some(0)),
        ("burst", 1000, 1500, Some(900)),
        ("missing", 900, 0, None),
        ("bad-first", 900, 5000, Some(5001)),
        ("too-long", 900, 3_600_001, Some(0)),
    ] {
        add(&s, id, AT + 20, output, duration, first);
    }
    s.set_meta(
        "usage_display",
        &UsageDisplaySettings {
            speed_calculation: Some("end_to_end".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let before = (
        s.records("all").unwrap(),
        s.timings().unwrap(),
        s.pending().unwrap(),
        s.usage_version().unwrap(),
    );
    let value = read(&s, json!({}));
    let speed = &value["summary"]["speed"];
    assert_eq!(speed["sampleCount"], 8);
    assert_eq!(speed["centralLower"], 20.0);
    assert_eq!(speed["centralUpper"], 70.0);
    assert_eq!(speed["centralSampleCount"], 6);
    assert_eq!(speed["min"], 10.0);
    assert_eq!(speed["max"], 800.0);
    assert!((speed["mean"].as_f64().unwrap() - 37600.0 / 180.0).abs() < 1e-9);
    assert_eq!(value["summary"]["ttft"]["sampleCount"], 8);
    assert_eq!(value["summary"]["ttft"]["centralSampleCount"], 8); // ties count as covered
    assert_eq!(value["excludedRecordCount"], 6);
    assert_eq!(value["buckets"][1]["speed"]["mean"], Value::Null);
    assert_eq!(
        before,
        (
            s.records("all").unwrap(),
            s.timings().unwrap(),
            s.pending().unwrap(),
            s.usage_version().unwrap()
        )
    );
}

#[test]
fn filters_devices_boundaries_and_empty_buckets_share_usage_selection() {
    let tmp = tempfile::tempdir().unwrap();
    let s = Store::open(&tmp.path().join("scope.db")).unwrap();
    add(&s, "a", AT, 400, 3000, Some(1000));
    add(&s, "b", AT + 300, 300, 4000, Some(1000));
    add(&s, "end", AT + 1199, 600, 3000, Some(1000));
    add(&s, "outside", AT + 1200, 800, 3000, Some(1000));
    let local = read(&s, json!({}));
    assert_eq!(local["buckets"].as_array().unwrap().len(), 4);
    assert_eq!(local["summary"]["speed"]["sampleCount"], 3);
    assert_eq!(local["buckets"][0]["speed"]["centralLower"], 200.0);
    assert_eq!(local["buckets"][1]["speed"]["sampleCount"], 1);
    assert_eq!(local["buckets"][2]["speed"]["mean"], Value::Null);
    assert_eq!(local["buckets"][3]["end"], AT + 1200);
    let mut remote = record(&s, "remote", AT + 20, 1000);
    remote.device_id = "remote-device".into();
    let timing = UsageTiming {
        duration_ms: 6000,
        first_token_ms: Some(1000),
        duration_source: "fixture".into(),
        first_token_source: Some("fixture".into()),
    };
    s.conn
        .execute(
            "INSERT INTO remote_records VALUES(?1,?2,1,?3)",
            rusqlite::params![
                remote.device_id,
                remote.id,
                serde_json::to_string(&remote).unwrap()
            ],
        )
        .unwrap();
    s.conn
        .execute(
            "INSERT INTO record_timings(device_id,id,payload) VALUES(?1,?2,?3)",
            rusqlite::params![
                remote.device_id,
                remote.id,
                serde_json::to_string(&timing).unwrap()
            ],
        )
        .unwrap();
    assert_eq!(
        read(&s, json!({"scope":"all"}))["summary"]["speed"]["sampleCount"],
        4
    );
    assert_eq!(
        read(&s, json!({"scope":"remote-device"}))["summary"]["speed"]["sampleCount"],
        1
    );
    assert_eq!(
        read(&s, json!({"scope":"local"}))["summary"]["speed"]["sampleCount"],
        3
    );
    for filter in [
        json!({"appType":"claude"}),
        json!({"providerName":"other"}),
        json!({"model":"other"}),
    ] {
        let empty = read(&s, filter);
        assert_eq!(empty["summary"]["speed"]["sampleCount"], 0);
        assert!(empty["buckets"]
            .as_array()
            .unwrap()
            .iter()
            .all(|b| b["speed"]["mean"].is_null()));
    }
    let mut config = s.config().unwrap();
    config.timezone = "America/New_York".into();
    s.set_meta("config", &config).unwrap();
    assert_eq!(read(&s, json!({})), local); // fixed elapsed-time UTC bins, display uses selected zone
    let clipped = read(&s, json!({"startDate":AT+1,"endDate":AT+300}));
    assert_eq!(clipped["summary"]["speed"]["sampleCount"], 1);
    assert_eq!(clipped["buckets"][0]["start"], AT + 1);
    assert_eq!(clipped["buckets"][1]["end"], AT + 301);
}

#[test]
fn all_time_is_bounded_and_invalid_ranges_fail_without_allocating() {
    let tmp = tempfile::tempdir().unwrap();
    let s = Store::open(&tmp.path().join("scope.db")).unwrap();
    let empty = read(&s, json!({"startDate":0}));
    assert!(empty["buckets"].as_array().unwrap().len() <= 241);
    add(&s, "old", AT - 10 * 365 * 86400, 400, 3000, Some(1000));
    let all = read(&s, json!({"startDate":0}));
    assert_eq!(all["summary"]["speed"]["sampleCount"], 1);
    assert_eq!(all["startDate"], AT - 10 * 365 * 86400);
    assert!(all["buckets"].as_array().unwrap().len() <= 241);
    for args in [
        json!({"startDate":2,"endDate":1}),
        json!({"startDate":-1,"endDate":1}),
        json!({"startDate":0,"endDate":i64::MAX}),
    ] {
        assert!(query::query(&s, "speedTrends", &args).is_err());
    }
}

#[test]
fn daily_means_use_strict_samples_and_configured_calendar_dates() {
    let tmp = tempfile::tempdir().unwrap();
    let s = Store::open(&tmp.path().join("scope.db")).unwrap();
    let at = chrono::DateTime::parse_from_rfc3339("2026-11-08T15:59:59Z")
        .unwrap()
        .timestamp();
    let mut config = s.config().unwrap();
    config.timezone = "Asia/Shanghai".into();
    s.set_meta("config", &config).unwrap();
    s.set_meta("usage_display", &json!({"speedCalculation":"end_to_end"}))
        .unwrap();
    add(&s, "before-midnight", at, 200, 11000, Some(1000));
    add(&s, "midnight", at + 1, 400, 21000, Some(1000));
    add(&s, "fast", at + 2, 1000, 10000, Some(0));
    add(&s, "e2e", at + 3, 1000, 10000, None);
    add(&s, "short", at + 4, 100, 10000, Some(0));
    add(&s, "no-valid-day", at + 86401, 500, 10000, None);
    let before = (
        s.records("all").unwrap(),
        s.timings().unwrap(),
        s.pending().unwrap(),
        s.usage_version().unwrap(),
    );
    let args = json!({"startDate":0,"endDate":at+86401,"scope":"all"});
    let value = query::query(&s, "speedDaily", &args).unwrap();
    assert_eq!(value["days"].as_array().unwrap().len(), 2);
    assert_eq!(value["days"][0]["date"], "2026-11-08");
    let day = &value["days"][1];
    assert_eq!(day["date"], "2026-11-09");
    assert_eq!(day["sampleCount"], 2);
    assert_eq!(day["outputTokens"], 1400.0);
    assert_eq!(day["generationMs"], 30000.0); // 46.67 tok/s, not mean(20,100)=60
    assert_eq!(day["firstTokenMs"], 1000.0); // 0.5 seconds for the same two samples
    assert_eq!(value["excludedRecordCount"], 3);
    assert_eq!(
        before,
        (
            s.records("all").unwrap(),
            s.timings().unwrap(),
            s.pending().unwrap(),
            s.usage_version().unwrap()
        )
    );
    for filter in [
        json!({"appType":"claude"}),
        json!({"providerName":"other"}),
        json!({"model":"other"}),
        json!({"scope":"missing-device"}),
    ] {
        let mut filtered = args.clone();
        filtered
            .as_object_mut()
            .unwrap()
            .extend(filter.as_object().unwrap().clone());
        assert_eq!(
            query::query(&s, "speedDaily", &filtered).unwrap()["days"],
            json!([])
        );
    }
    let bounded =
        query::query(&s, "speedDaily", &json!({"startDate":at+1,"endDate":at+1})).unwrap();
    assert_eq!(bounded["days"][0]["sampleCount"], 1);
    assert!(query::query(&s, "speedDaily", &json!({"startDate":0,"endDate":i64::MAX})).is_err());
}

#[test]
fn daily_calendar_handles_dst_repeated_hour_and_decades_without_coarsening() {
    let tmp = tempfile::tempdir().unwrap();
    let s = Store::open(&tmp.path().join("scope.db")).unwrap();
    let mut config = s.config().unwrap();
    config.timezone = "America/New_York".into();
    s.set_meta("config", &config).unwrap();
    for (i, stamp) in [
        "2010-01-02T12:00:00Z",
        "2026-11-01T05:30:00Z",
        "2026-11-01T06:30:00Z",
        "2026-11-02T05:00:00Z",
    ]
    .iter()
    .enumerate()
    {
        let at = chrono::DateTime::parse_from_rfc3339(stamp)
            .unwrap()
            .timestamp();
        add(&s, &format!("dst-{i}"), at, 400, 3000, Some(1000));
    }
    let value = query::query(
        &s,
        "speedDaily",
        &json!({"startDate":0,"endDate":1800000000}),
    )
    .unwrap();
    let days = value["days"].as_array().unwrap();
    assert_eq!(days.len(), 3);
    assert_eq!(days[0]["date"], "2010-01-02");
    assert_eq!(days[1]["date"], "2026-11-01");
    assert_eq!(days[1]["sampleCount"], 2);
    assert_eq!(days[2]["date"], "2026-11-02");
}
