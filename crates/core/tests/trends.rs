use chrono::DateTime;
use harness_scope_core::{model::*, Engine};
use serde_json::{json, Value};

fn ts(s: &str) -> i64 {
    DateTime::parse_from_rfc3339(s).unwrap().timestamp()
}
fn engine(dir: &std::path::Path, zone: &str) -> Engine {
    let e = Engine::open(&dir.join("scope.db")).unwrap();
    {
        let s = e.lock().unwrap();
        let mut c = s.config().unwrap();
        c.timezone = zone.into();
        c.sources.clear();
        s.set_meta("config", &c).unwrap();
    }
    e
}
fn add(e: &Engine, id: &str, at: i64, harness: &str) {
    let s = e.lock().unwrap();
    s.upsert(
        UsageRecord {
            timing: None,
            request_metadata: None,
            provider: None,
            device_id: s.device_id().unwrap(),
            id: id.into(),
            revision: 1,
            harness: harness.into(),
            parser_version: "test".into(),
            session_id: Some("session".into()),
            native_event_id: Some(id.into()),
            occurred_at: at,
            model: "fixture".into(),
            grain: "event".into(),
            count: 1,
            tokens: Tokens {
                input: 10,
                output: 2,
                cache_read: 4,
                cache_write: 3,
                ..Default::default()
            },
            cost: Cost {
                amount_usd: Some("0.25".into()),
                method: "reported".into(),
                ..Default::default()
            },
            record_source: "session".into(),
            deleted: false,
            status_code: None,
            legacy_date: None,
            legacy_timezone: None,
        },
        "synthetic",
    )
    .unwrap();
}
fn query(e: &Engine, kind: &str, start: i64, end: i64, harness: &str) -> Value {
    e.execute(
        "usage_query",
        json!({"kind":kind,"startDate":start,"endDate":end,"appType":harness}),
    )
    .unwrap()
}
fn check_totals(rows: &[Value], summary: &Value) {
    for (column, total) in [
        ("requestCount", "totalRequests"),
        ("totalInputTokens", "totalInputTokens"),
        ("totalOutputTokens", "totalOutputTokens"),
        ("totalCacheReadTokens", "totalCacheReadTokens"),
        ("totalCacheCreationTokens", "totalCacheCreationTokens"),
        ("totalTokens", "realTotalTokens"),
    ] {
        assert_eq!(
            rows.iter()
                .map(|r| r[column].as_u64().unwrap())
                .sum::<u64>(),
            summary[total].as_u64().unwrap(),
            "{column}"
        );
    }
    let cost: rust_decimal::Decimal = rows
        .iter()
        .map(|r| {
            r["totalCost"]
                .as_str()
                .unwrap()
                .parse::<rust_decimal::Decimal>()
                .unwrap()
        })
        .sum();
    assert_eq!(
        cost,
        summary["totalCost"]
            .as_str()
            .unwrap()
            .parse::<rust_decimal::Decimal>()
            .unwrap()
    );
}

#[test]
fn today_keeps_midnight_and_empty_hours_through_now_including_filtered_empty_ranges() {
    let tmp = tempfile::tempdir().unwrap();
    let e = engine(tmp.path(), "Asia/Shanghai");
    let start = ts("2026-10-19T00:00:00+08:00");
    let end = ts("2026-10-19T10:17:00+08:00");
    for (id, at) in [
        ("before", start - 1),
        ("early", start + 3 * 3600),
        ("later", start + 8 * 3600),
        ("after", end + 1),
    ] {
        add(&e, id, at, "codex");
    }
    let before = e.lock().unwrap().local_records().unwrap();
    let rows = query(&e, "trends", start, end, "codex");
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 11);
    assert_eq!(rows[0]["date"], "2026-10-19T00:00:00+08:00");
    assert_eq!(rows[10]["date"], "2026-10-19T10:00:00+08:00");
    assert_eq!(rows[3]["requestCount"], 1);
    assert_eq!(rows[8]["requestCount"], 1);
    for i in [0, 1, 2, 4, 5, 6, 7, 9, 10] {
        assert_eq!(rows[i]["requestCount"], 0);
        assert_eq!(rows[i]["totalTokens"], 0);
        assert_eq!(rows[i]["totalCost"], "0");
    }
    check_totals(rows, &query(&e, "summary", start, end, "codex"));
    let empty = query(&e, "trends", start, end, "dsh");
    assert_eq!(empty.as_array().unwrap().len(), 11);
    assert!(empty
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["requestCount"] == 0));
    let midnight = query(&e, "trends", start, start, "codex");
    assert_eq!(midnight.as_array().unwrap().len(), 1);
    assert_eq!(midnight[0]["date"], rows[0]["date"]);
    assert_eq!(before, e.lock().unwrap().local_records().unwrap());
}

#[test]
fn rolling_hours_anchor_to_range_start_and_keep_the_inclusive_end() {
    let tmp = tempfile::tempdir().unwrap();
    let e = engine(tmp.path(), "Asia/Kathmandu");
    let start = ts("2026-10-18T13:23:45+05:45");
    let end = start + 86400;
    for (id, at) in [("start", start), ("edge", start + 3600), ("end", end)] {
        add(&e, id, at, "dsh");
    }
    let rows = query(&e, "trends", start, end, "all");
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 24);
    assert_eq!(rows[0]["date"], "2026-10-18T13:23:45+05:45");
    assert_eq!(rows[1]["requestCount"], 1);
    assert_eq!(rows[23]["requestCount"], 1);
    check_totals(rows, &query(&e, "summary", start, end, "all"));
    let default = e.execute("usage_query", json!({"kind":"trends"})).unwrap();
    assert_eq!(default.as_array().unwrap().len(), 24);
    assert!(e
        .execute(
            "usage_query",
            json!({"kind":"trends","startDate":end,"endDate":start})
        )
        .is_err());
}

#[test]
fn daily_buckets_cover_empty_calendar_days_and_cross_years() {
    let tmp = tempfile::tempdir().unwrap();
    let e = engine(tmp.path(), "Pacific/Kiritimati");
    let start = ts("2025-12-29T00:00:00+14:00");
    let end = ts("2026-01-04T12:00:00+14:00");
    add(&e, "first", start, "claude");
    add(&e, "last", end, "claude");
    let rows = query(&e, "trends", start, end, "all");
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 7);
    assert_eq!(rows[3]["date"], "2026-01-01T00:00:00+14:00");
    assert!(rows[1..6].iter().all(|r| r["requestCount"] == 0));
    check_totals(rows, &query(&e, "summary", start, end, "all"));
}

#[test]
fn daylight_saving_repeated_hours_have_unique_offsets_and_days_keep_calendar_boundaries() {
    let tmp = tempfile::tempdir().unwrap();
    let e = engine(tmp.path(), "America/New_York");
    let start = ts("2026-11-01T00:00:00-04:00");
    let end = ts("2026-11-01T04:30:00-05:00");
    add(&e, "first-one", ts("2026-11-01T01:15:00-04:00"), "codex");
    add(&e, "second-one", ts("2026-11-01T01:15:00-05:00"), "codex");
    let rows = query(&e, "trends", start, end, "codex");
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 6);
    assert_eq!(rows[1]["date"], "2026-11-01T01:00:00-04:00");
    assert_eq!(rows[2]["date"], "2026-11-01T01:00:00-05:00");
    assert_eq!(rows[1]["requestCount"], 1);
    assert_eq!(rows[2]["requestCount"], 1);
    check_totals(rows, &query(&e, "summary", start, end, "codex"));
    let days = query(
        &e,
        "trends",
        ts("2026-03-06T00:00:00-05:00"),
        ts("2026-03-10T12:00:00-04:00"),
        "all",
    );
    let days = days.as_array().unwrap();
    assert_eq!(days.len(), 5);
    assert_eq!(days[2]["date"], "2026-03-08T00:00:00-05:00");
    assert_eq!(days[3]["date"], "2026-03-09T00:00:00-04:00");
    assert_eq!(
        ts(days[3]["date"].as_str().unwrap()) - ts(days[2]["date"].as_str().unwrap()),
        23 * 3600
    );
}

#[test]
fn legacy_rollups_only_contribute_when_the_complete_day_is_selected() {
    let tmp = tempfile::tempdir().unwrap();
    let e = engine(tmp.path(), "UTC");
    let day = ts("2026-10-18T00:00:00Z");
    add(&e, "legacy", day + 12 * 3600, "codex");
    {
        let s = e.lock().unwrap();
        let mut r = s.record("legacy").unwrap().unwrap();
        r.grain = "legacy_day".into();
        r.record_source = "migration".into();
        r.count = 7;
        r.legacy_date = Some("2026-10-18".into());
        r.legacy_timezone = Some("UTC".into());
        s.upsert(r, "synthetic").unwrap();
    }
    let rows = query(&e, "trends", day - 86400, day + 86400 - 1, "all");
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["requestCount"], 0);
    assert_eq!(rows[1]["requestCount"], 7);
    check_totals(
        rows,
        &query(&e, "summary", day - 86400, day + 86400 - 1, "all"),
    );
    let partial = query(&e, "trends", day + 12 * 3600, day + 13 * 3600, "all");
    assert_eq!(partial[0]["requestCount"], 0);
    {
        let s = e.lock().unwrap();
        let mut config = s.config().unwrap();
        config.timezone = "Pacific/Kiritimati".into();
        s.set_meta("config", &config).unwrap();
        let mut r = s.record("legacy").unwrap().unwrap();
        r.occurred_at = ts("2026-01-01T00:00:00-12:00");
        r.legacy_date = Some("2026-01-01".into());
        r.legacy_timezone = Some("Etc/GMT+12".into());
        s.upsert(r, "synthetic").unwrap();
    }
    let start = ts("2026-01-02T00:00:00+14:00");
    let end = ts("2026-01-03T23:59:59+14:00");
    let shifted = query(&e, "trends", start, end, "all");
    let shifted = shifted.as_array().unwrap();
    assert_eq!(shifted[0]["requestCount"], 7);
    check_totals(shifted, &query(&e, "summary", start, end, "all"));
}
