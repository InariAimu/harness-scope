use harness_scope_core::{
    harness::{codex::Codex, opencode::OpenCode, HarnessAdapter, TimelineIndex},
    model::*,
    query,
    store::Store,
    Engine,
};
use serde_json::{json, Value};
use std::path::Path;

const BASE: i64 = 1_792_112_400_000;
fn event(ms: i64, kind: &str, payload: Value) -> Value {
    json!({"timestamp":chrono::DateTime::from_timestamp_millis(BASE+ms).unwrap().to_rfc3339(),"type":kind,"payload":payload})
}
fn meta() -> Value {
    event(0, "session_meta", json!({"id":"timing","model":"gpt-5.4"}))
}
fn start(ms: i64) -> Value {
    event(ms, "event_msg", json!({"type":"task_started"}))
}
fn output(ms: i64) -> Value {
    event(ms, "response_item", json!({"type":"reasoning"}))
}
fn first(ms: i64, began: i64) -> Value {
    event(
        ms,
        "event_msg",
        json!({"type":"item_completed","item":{"type":"Reasoning"},"started_at_ms":BASE+began}),
    )
}
fn usage(ms: i64, total: u64) -> Value {
    event(
        ms,
        "event_msg",
        json!({"type":"token_count","info":{"total_token_usage":{"input_tokens":100*total,"output_tokens":300*total},"last_token_usage":{"input_tokens":100,"output_tokens":300}}}),
    )
}
fn codex(lines: &[Value]) -> Vec<ParsedUsage> {
    let parsed = Codex
        .parse(Path::new("synthetic.jsonl"), lines, &TimelineIndex::new())
        .unwrap();
    assert!(parsed.complete, "{:?}", parsed.diagnostics);
    parsed.usage
}
#[test]
fn codex_prefers_response_end_and_earliest_model_item() {
    let rows = codex(&[
        meta(),
        start(1000),
        first(3500, 2000),
        output(3500),
        first(4000, 2500),
        event(
            5000,
            "token_usage_record",
            json!({"usage":{"input_tokens":100,"output_tokens":300}}),
        ),
        event(
            100_000,
            "response_item",
            json!({"type":"function_call_output"}),
        ),
        usage(100_001, 1),
    ]);
    let t = rows[0].timing.as_ref().unwrap();
    assert_eq!((t.duration_ms, t.first_token_ms), (4000, Some(1000)));
    assert_eq!(t.duration_source, "codex_token_usage_record");
    assert_eq!(rows[0].id, "codex:timing:token:1");
}
#[test]
fn codex_excludes_tool_time_and_does_not_reuse_aborted_or_flushed_output() {
    let rows = codex(&[
        meta(),
        start(1000),
        output(4000),
        event(
            100_000,
            "response_item",
            json!({"type":"function_call_output"}),
        ),
        usage(100_001, 1),
        output(100_001), // old layout: output was flushed after token_count
        event(
            200_000,
            "response_item",
            json!({"type":"message","role":"user"}),
        ),
        output(202_000),
        usage(203_000, 2),
        start(204_000),
        output(205_000),
        event(210_000, "event_msg", json!({"type":"turn_aborted"})),
        start(300_000),
        first(302_000, 301_000),
        output(302_000),
        usage(304_000, 3),
    ]);
    let ts: Vec<_> = rows.iter().map(|r| r.timing.as_ref().unwrap()).collect();
    assert_eq!(
        ts.iter().map(|t| t.duration_ms).collect::<Vec<_>>(),
        vec![3000, 3000, 4000]
    );
    assert_eq!(ts[0].duration_source, "codex_model_output_before_tool");
    assert_eq!(ts[0].first_token_ms, None);
    assert_eq!(ts[2].first_token_ms, Some(1000));
}
#[test]
fn codex_repeated_snapshots_do_not_split_next_request_or_shift_ids() {
    let rows = codex(&[
        meta(),
        start(1000),
        output(3000),
        usage(4000, 1),
        event(
            5000,
            "response_item",
            json!({"type":"function_call_output"}),
        ),
        usage(6000, 1),
        first(8000, 7000),
        output(8000),
        usage(9000, 2),
    ]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].id, "codex:timing:token:3");
    let t = rows[1].timing.as_ref().unwrap();
    assert_eq!((t.duration_ms, t.first_token_ms), (4000, Some(2000)));
    assert!(codex(&[meta(), usage(9000, 1)])[0].timing.is_none());
}

fn oc(time: Value, content: Value) -> Value {
    json!({"kind":"message","id":"msg","sessionId":"ses","data":{"role":"assistant","model":{"id":"gpt-5.4"},"time":time,"tokens":{"input":100,"output":300},"content":content}})
}
fn opencode(events: &[Value]) -> ParsedUsage {
    let mut parsed = OpenCode
        .parse(Path::new("synthetic.db"), events, &TimelineIndex::new())
        .unwrap();
    assert!(parsed.complete, "{:?}", parsed.diagnostics);
    parsed.usage.remove(0)
}
#[test]
fn opencode_v2_stream_end_excludes_tools_and_content_start_estimates_first_token() {
    let row = opencode(&[oc(
        json!({"created":BASE,"streamed":BASE+5000,"completed":BASE+120_000}),
        json!([{"type":"tool","time":{"created":BASE+100}},
               {"type":"text","time":{"created":BASE+3000}},
               {"type":"reasoning","time":{"created":BASE+2000}}]),
    )]);
    let t = row.timing.unwrap();
    assert_eq!((t.duration_ms, t.first_token_ms), (5000, Some(2000)));
    assert_eq!(t.duration_source, "opencode_time_streamed");
}
#[test]
fn opencode_falls_back_to_completion_without_inventing_first_token() {
    for streamed in [Value::Null, json!(BASE - 1)] {
        let row = opencode(&[oc(
            json!({"created":BASE,"streamed":streamed,"completed":BASE+8000}),
            json!([{"type":"text"}]),
        )]);
        let t = row.timing.unwrap();
        assert_eq!((t.duration_ms, t.first_token_ms), (8000, None));
        assert_eq!(t.duration_source, "opencode_time_completed");
    }
    let row = opencode(&[oc(json!({"created":BASE,"completed":BASE-1}), json!([]))]);
    assert!(row.timing.is_none());
    assert_eq!(row.tokens.output, 300); // malformed optional timing cannot drop billing facts
}
#[test]
fn opencode_v1_parts_are_scoped_to_message_and_session() {
    let row = opencode(&[
        oc(json!({"created":BASE,"completed":BASE+8000}), json!([])),
        json!({"kind":"part","messageId":"msg","sessionId":"other","data":{"type":"reasoning","time":{"start":BASE+100}}}),
        json!({"kind":"part","messageId":"msg","sessionId":"ses","data":{"type":"tool","time":{"start":BASE+200}}}),
        json!({"kind":"part","messageId":"msg","sessionId":"ses","data":{"type":"reasoning","time":{"start":BASE+2000}}}),
    ]);
    assert_eq!(row.timing.unwrap().first_token_ms, Some(2000));
}
#[test]
fn implausible_durations_and_out_of_range_first_tokens_are_unknown() {
    for delta in [-1, 0, 99, 3_600_001] {
        assert!(UsageTiming::estimate(BASE, BASE + delta, None, "test", "test").is_none());
    }
    for first in [BASE - 1, BASE + 1001] {
        assert_eq!(
            UsageTiming::estimate(BASE, BASE + 1000, Some(first), "test", "test")
                .unwrap()
                .first_token_ms,
            None
        );
    }
}

#[test]
fn backfill_preserves_cost_and_tokens_and_is_transactional() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("logs");
    std::fs::create_dir(&path).unwrap();
    let rows = [
        meta(),
        start(1000),
        first(3000, 2000),
        output(3000),
        usage(5000, 1),
    ];
    std::fs::write(
        path.join("session.jsonl"),
        rows.iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    let db = tmp.path().join("scope.db");
    let e = Engine::open(&db).unwrap();
    {
        let s = e.lock().unwrap();
        let mut c = s.config().unwrap();
        c.sources = vec![Source {
            id: "fixture".into(),
            harness: "codex".into(),
            root: path.to_string_lossy().into(),
            enabled: true,
        }];
        s.set_meta("config", &c).unwrap();
    }
    assert_eq!(e.scan(false).unwrap().imported, 1);
    let before = {
        let s = e.lock().unwrap();
        s.conn.execute("DELETE FROM record_timings", []).unwrap();
        s.conn
            .execute("UPDATE file_state SET parser_version='codex-v2'", [])
            .unwrap();
        s.local_records().unwrap().remove(0)
    };
    e.scan(false).unwrap();
    let s = e.lock().unwrap();
    let after = s.local_records().unwrap().remove(0);
    assert_eq!(before, after); // timing does not rewrite price snapshots, counters or revision
    assert_eq!(s.timings().unwrap().len(), 1);
    assert!(serde_json::to_string(&s.pending().unwrap())
        .unwrap()
        .contains("durationMs"));
    let failed: anyhow::Result<()> = s.transaction(|| {
        s.save_timing(&before.id, None)?;
        anyhow::bail!("rollback")
    });
    assert!(failed.is_err());
    assert_eq!(s.timings().unwrap().len(), 1);
    drop(s);
    drop(e);
    assert_eq!(Store::open(&db).unwrap().timings().unwrap().len(), 1);
}

#[test]
fn stats_use_weighted_speed_median_filters_and_device_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let s = Store::open(&tmp.path().join("scope.db")).unwrap();
    let device = s.device_id().unwrap();
    for (id, output, duration, first) in [
        ("a", 300, 1000, 100),
        ("b", 900, 9000, 300),
        ("small", 199, 2000, 200),
        ("short", 300, 999, 400),
        ("missing", 999, 0, 0),
    ] {
        let record = UsageRecord {
            timing: None,
            request_metadata: None,
            provider: None,
            device_id: device.clone(),
            id: id.into(),
            harness: "codex".into(),
            parser_version: "test".into(),
            session_id: Some("s".into()),
            native_event_id: Some(id.into()),
            occurred_at: BASE / 1000,
            model: "test-model".into(),
            grain: "event".into(),
            count: 1,
            tokens: Tokens {
                output,
                ..Default::default()
            },
            cost: Cost::default(),
            record_source: "session".into(),
            revision: 1,
            deleted: false,
            status_code: None,
            legacy_date: None,
            legacy_timezone: None,
        };
        s.upsert(record.clone(), "fixture").unwrap();
        if duration > 0 {
            s.save_timing(
                id,
                UsageTiming::estimate(BASE, BASE + duration, Some(BASE + first), "test", "test")
                    .as_ref(),
            )
            .unwrap();
        }
        if id == "a" {
            let mut remote = record;
            remote.device_id = "remote".into();
            s.conn
                .execute(
                    "INSERT INTO remote_records VALUES(?1,?2,?3,?4)",
                    rusqlite::params![
                        remote.device_id,
                        remote.id,
                        1,
                        serde_json::to_string(&remote).unwrap()
                    ],
                )
                .unwrap();
        }
    }
    for kind in ["models", "providers"] {
        let stats = query::query(&s, kind, &json!({"scope":"all"})).unwrap();
        let row = &stats[0];
        assert_eq!(row["estSpeedOutputTokens"], 1200);
        assert_eq!(row["estSpeedDurationMs"], 10_000);
        assert_eq!(row["estSpeedSampleCount"], 2);
        assert_eq!(row["estFirstTokenMedianMs"], 250.0);
        assert_eq!(row["estFirstTokenSampleCount"], 4);
        assert!(row["successRate"].is_null());
        assert!(query::query(&s, kind, &json!({"startDate":BASE/1000+1}))
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty());
    }
    let remote =
        query::query(&s, "detail", &json!({"scope":"all","requestId":"remote|a"})).unwrap();
    assert!(remote["latencyMs"].is_null());
    let local = query::query(&s, "detail", &json!({"requestId":format!("{device}|a")})).unwrap();
    assert_eq!(local["timingMethod"], "estimated");
    assert_eq!(local["firstTokenMs"], 100);
}

#[test]
fn dsh_decode_speed_is_consistent_in_details_and_mixed_harness_statistics() {
    let tmp = tempfile::tempdir().unwrap();
    let s = Store::open(&tmp.path().join("scope.db")).unwrap();
    let device = s.device_id().unwrap();
    for (id, harness, output, duration, first, first_source) in [
        ("a", "dsh", 488, 6846, Some(4364), "dsh_stream_delta"),
        ("b", "dsh", 122, 10284, Some(9790), "dsh_stream_delta"),
        ("boundary", "dsh", 200, 2000, Some(1000), "dsh_stream_delta"),
        ("almost", "dsh", 200, 2000, Some(1001), "dsh_stream_delta"),
        ("few", "dsh", 199, 4000, Some(1000), "dsh_stream_delta"),
        ("spike775", "dsh", 124, 6491, Some(6331), "dsh_stream_delta"),
        (
            "buffered",
            "dsh",
            1311,
            8836,
            Some(8058),
            "dsh_stream_delta",
        ),
        ("burst", "dsh", 300, 6000, Some(5950), "dsh_stream_delta"),
        ("tiny", "dsh", 99, 6000, Some(1000), "dsh_stream_delta"),
        ("missing", "dsh", 300, 4000, None, "dsh_stream_delta"),
        ("untrusted", "dsh", 300, 6000, Some(1000), "unknown"),
        (
            "codex",
            "codex",
            300,
            8000,
            Some(2000),
            "codex_item_started_at_ms",
        ),
        (
            "opencode",
            "opencode",
            500,
            6000,
            Some(1000),
            "opencode_content_start",
        ),
        ("claude", "claude", 300, 0, None, "unknown"),
    ] {
        let record = UsageRecord {
            timing: None,
            request_metadata: None,
            provider: None,
            device_id: device.clone(),
            id: id.into(),
            harness: harness.into(),
            parser_version: "test".into(),
            session_id: Some("s".into()),
            native_event_id: Some(id.into()),
            occurred_at: BASE / 1000,
            model: "mixed-model".into(),
            grain: "event".into(),
            count: 1,
            tokens: Tokens {
                output,
                ..Default::default()
            },
            cost: Cost::default(),
            record_source: "session".into(),
            revision: 1,
            deleted: false,
            status_code: None,
            legacy_date: None,
            legacy_timezone: None,
        };
        s.upsert(record.clone(), "fixture").unwrap();
        if duration > 0 {
            s.save_timing(
                id,
                UsageTiming::estimate(
                    BASE,
                    BASE + duration,
                    first.map(|v| BASE + v),
                    if harness == "dsh" {
                        "dsh_step_message"
                    } else {
                        "test"
                    },
                    first_source,
                )
                .as_ref(),
            )
            .unwrap();
        }
        if id == "a" {
            let mut remote = record;
            remote.device_id = "remote".into();
            s.conn
                .execute(
                    "INSERT INTO remote_records VALUES(?1,?2,?3,?4)",
                    rusqlite::params![
                        remote.device_id,
                        remote.id,
                        1,
                        serde_json::to_string(&remote).unwrap()
                    ],
                )
                .unwrap();
        }
    }
    let before = s.records("all").unwrap();
    let before_pending = s.pending().unwrap();
    let before_version = s.usage_version().unwrap();
    let before_timings = s.timings().unwrap();
    for (id, duration, eligible) in [
        ("a", 2482, true),
        ("b", 494, false),
        ("boundary", 1000, true),
        ("almost", 999, false),
        ("few", 3000, false),
        ("spike775", 160, false),
        ("buffered", 778, false),
        ("burst", 50, false),
        ("tiny", 5000, false),
    ] {
        let detail =
            query::query(&s, "detail", &json!({"requestId":format!("{device}|{id}")})).unwrap();
        assert_eq!(detail["estimatedSpeed"]["basis"], "decode");
        assert_eq!(detail["estimatedSpeed"]["durationMs"], duration);
        assert_eq!(detail["estimatedSpeed"]["eligible"], eligible);
        assert_eq!(detail["estimatedSpeed"]["minOutputTokens"], 200);
        assert_eq!(detail["estimatedSpeed"]["minDurationMs"], 1000);
        let logs = query::query(&s, "logs", &json!({"pageSize":100})).unwrap();
        let log = logs["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|log| log["requestId"] == detail["requestId"])
            .unwrap();
        assert_eq!(log["estimatedSpeed"], detail["estimatedSpeed"]);
        assert_eq!(log["outputTokens"], detail["outputTokens"]);
        assert_eq!(log["firstTokenMs"], detail["firstTokenMs"]);
    }
    for id in ["missing", "untrusted", "codex", "opencode"] {
        let detail =
            query::query(&s, "detail", &json!({"requestId":format!("{device}|{id}")})).unwrap();
        assert_eq!(detail["estimatedSpeed"]["basis"], "end_to_end");
        assert_eq!(detail["estimatedSpeed"]["eligible"], true);
    }
    for request in ["remote|a".to_string(), format!("{device}|claude")] {
        let detail =
            query::query(&s, "detail", &json!({"scope":"all","requestId":request})).unwrap();
        assert!(detail["estimatedSpeed"].is_null());
    }
    let stats = query::query(&s, "models", &json!({"scope":"all"})).unwrap();
    assert_eq!(stats[0]["estDecodeSpeedOutputTokens"], 688);
    assert_eq!(stats[0]["estDecodeSpeedGenerationMs"], 3482);
    assert_eq!(stats[0]["estDecodeSpeedSampleCount"], 2);
    assert_eq!(stats[0]["estSpeedOutputTokens"], 1400);
    assert_eq!(stats[0]["estSpeedDurationMs"], 24000);
    assert_eq!(stats[0]["estSpeedSampleCount"], 4);
    let stats = query::query(&s, "providers", &json!({"filters":{"appType":"dsh"}})).unwrap();
    assert_eq!(stats[0]["estDecodeSpeedOutputTokens"], 688);
    assert_eq!(stats[0]["estDecodeSpeedGenerationMs"], 3482);
    assert_eq!(stats[0]["estDecodeSpeedSampleCount"], 2);
    assert_eq!(stats[0]["estSpeedOutputTokens"], 600);
    assert_eq!(stats[0]["estSpeedDurationMs"], 10000);
    assert_eq!(before, s.records("all").unwrap());
    assert_eq!(before_pending, s.pending().unwrap());
    assert_eq!(before_version, s.usage_version().unwrap());
    assert_eq!(before_timings, s.timings().unwrap());
}
