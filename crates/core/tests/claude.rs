use harness_scope_core::{
    harness::{claude::Claude, files, HarnessAdapter, ParseResult, TimelineIndex},
    model::{PushRequest, Source},
    store::Store,
    sync, Engine,
};
use serde_json::{json, Value};
use std::{fs, path::Path};

const MODERN: &str = include_str!("../../../tests/fixtures/claude/modern.jsonl");
fn events() -> Vec<Value> {
    MODERN
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect()
}
fn parse(events: &[Value]) -> ParseResult {
    Claude
        .parse(Path::new("synthetic.jsonl"), events, &TimelineIndex::new())
        .unwrap()
}

#[test]
fn request_blocks_exclude_tool_time_keep_reasoning_as_subset_and_ignore_cost_state() {
    let rows = parse(&events());
    assert!(rows.complete && rows.diagnostics.is_empty());
    assert_eq!(rows.usage.len(), 2);
    let first = &rows.usage[0];
    assert_eq!(first.id, "claude:claude-modern:message-one");
    assert_eq!(
        (
            first.tokens.output,
            first.tokens.reasoning,
            first.tokens.total()
        ),
        (1000, 800, 1602)
    );
    let t = first.timing.as_ref().unwrap();
    assert_eq!((t.duration_ms, t.first_token_ms), (3500, None));
    assert_eq!(t.duration_source, "claude_api_request_blocks");
    assert!(t.first_token_source.is_none());
    let m = first.request_metadata.as_ref().unwrap();
    assert_eq!(m.reasoning_effort.as_deref(), Some("max"));
    assert_eq!(m.service_tier.as_deref(), Some("standard"));
    assert_eq!(m.service_tier_source.as_deref(), Some("claude_usage"));
    assert!(first.provider.is_none() && first.reported_cost.is_none());
    assert_eq!(rows.usage[1].timing.as_ref().unwrap().duration_ms, 1500);
    let mut repeated = events();
    repeated.extend(events());
    assert_eq!(
        format!("{:?}", parse(&repeated).usage),
        format!("{:?}", rows.usage)
    );
}

#[test]
fn ambiguous_missing_or_implausible_boundaries_never_invent_timing() {
    for case in 0..8 {
        let mut rows = events();
        match case {
            0 => rows.retain(|v| v["type"] != "api-request"),
            1 => rows[2]["sessionId"] = json!("other-session"),
            2 => {
                let mut duplicate = rows[2].clone();
                duplicate["timestamp"] = json!("2026-10-21T01:00:00.200Z");
                rows.push(duplicate);
            }
            3 => rows[4]["requestRef"] = json!("different-request"),
            4 => {
                let mut duplicate = rows[4].clone();
                duplicate["message"]["id"] = json!("different-message");
                rows.push(duplicate);
            }
            5 => rows[2]["timestamp"] = json!("2026-10-21T01:00:03.600Z"),
            6 => rows[2]["timestamp"] = json!("2026-10-20T01:00:00.000Z"),
            7 => {
                rows[3]["message"]["stop_reason"] = Value::Null;
                rows[4]["message"]["stop_reason"] = Value::Null;
            }
            _ => unreachable!(),
        }
        let result = parse(&rows);
        assert!(result.complete, "case {case}: {:?}", result.diagnostics);
        let first = result
            .usage
            .iter()
            .find(|r| r.native_event_id == "message-one")
            .unwrap();
        assert!(first.timing.is_none(), "case {case}");
        assert_eq!(first.tokens.output, 1000);
    }
    let old: Vec<Value> = include_str!("../../../tests/fixtures/claude/session.jsonl")
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert!(parse(&old).usage[0].timing.is_none());
}

#[test]
fn invalid_reasoning_counters_remain_incomplete_and_request_settings_do_not_leak() {
    for bad in [
        json!(-1),
        json!(true),
        json!(2.5),
        json!("800"),
        json!(1001),
    ] {
        let mut rows = events();
        for row in &mut rows {
            if row["message"]["id"] == "message-one" {
                row["message"]["usage"]["output_tokens_details"]["thinking_tokens"] = bad.clone();
            }
        }
        let result = parse(&rows);
        assert!(!result.complete && !result.diagnostics.is_empty());
        assert_eq!(result.usage.len(), 1);
    }
    let mut rows = events();
    rows[6]["params"]["output_config"]["effort"] = json!("unsupported-private-setting");
    rows[7]["effort"] = json!("low");
    let result = parse(&rows);
    assert_eq!(
        result.usage[1]
            .request_metadata
            .as_ref()
            .unwrap()
            .reasoning_effort
            .as_deref(),
        Some("low")
    );
    rows[7]["effort"] = Value::Null;
    assert!(parse(&rows).usage[1]
        .request_metadata
        .as_ref()
        .unwrap()
        .reasoning_effort
        .is_none());
}

#[test]
fn projection_and_compression_keep_new_fields_and_readable_titles() {
    let tmp = tempfile::tempdir().unwrap();
    for compressed in [false, true] {
        let path = tmp.path().join(if compressed {
            "session.jsonl.zst"
        } else {
            "session.jsonl"
        });
        fs::write(
            &path,
            if compressed {
                zstd::stream::encode_all(MODERN.as_bytes(), 1).unwrap()
            } else {
                MODERN.as_bytes().to_vec()
            },
        )
        .unwrap();
        let (full, hash, errors, complete) = files::read(&path).unwrap();
        assert!(complete && errors.is_empty());
        let (projected, p_hash, errors, complete) = Claude.read_usage_source(&path).unwrap();
        assert!(complete && errors.is_empty());
        assert_eq!(hash, p_hash);
        assert_eq!(
            format!("{:?}", parse(&full).usage),
            format!("{:?}", parse(&projected).usage)
        );
        let metadata = Claude.read_metadata(&path).unwrap();
        assert_eq!(
            metadata[0].title.as_deref(),
            Some("Synthetic named conversation")
        );
        assert_eq!(
            metadata[0].summary.as_deref(),
            Some("Explain this synthetic request")
        );
    }
    let mut rows = events();
    rows.push(json!({"type":"custom-title","sessionId":"claude-modern","customTitle":"My title"}));
    rows.push(json!({"type":"ai-title","sessionId":"other-session","aiTitle":"Wrong title"}));
    assert_eq!(
        Claude
            .session(Path::new("s.jsonl"), &rows)
            .unwrap()
            .title
            .as_deref(),
        Some("My title")
    );
}

#[test]
fn upgrade_preserves_frozen_prices_revisions_and_syncs_estimates_without_first_tokens() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("logs");
    fs::create_dir(&root).unwrap();
    let path = root.join("synthetic.jsonl");
    fs::write(&path, MODERN).unwrap();
    let db = tmp.path().join("scope.db");
    let engine = Engine::open(&db).unwrap();
    {
        let s = engine.lock().unwrap();
        let mut config = s.config().unwrap();
        config.sources = vec![Source {
            id: "fixture".into(),
            harness: "claude".into(),
            root: root.to_string_lossy().into(),
            enabled: true,
        }];
        s.set_meta("config", &config).unwrap();
    }
    assert_eq!(engine.scan(false).unwrap().imported, 2);
    let before = {
        let s = engine.lock().unwrap();
        let mut rows = s.local_records().unwrap();
        for row in &mut rows {
            row.parser_version = "claude-v1".into();
            row.tokens.reasoning = 0;
            row.timing = None;
            row.request_metadata = None;
            row.cost.priced_at = 12345;
            assert!(row.cost.amount_usd.is_some());
            s.upsert(row.clone(), &path.to_string_lossy()).unwrap();
        }
        s.conn
            .execute("UPDATE file_state SET parser_version='claude-v1'", [])
            .unwrap();
        s.conn.execute("DELETE FROM usage_read_cache", []).unwrap();
        s.conn.execute("DELETE FROM outbox", []).unwrap();
        s.local_records().unwrap()
    };
    let report = engine.scan(false).unwrap();
    assert_eq!(
        (report.imported, report.updated, report.deferred_files),
        (0, 2, 0)
    );
    assert!(report.errors.is_empty());
    let after = engine.lock().unwrap().local_records().unwrap();
    for old in &before {
        let row = after.iter().find(|r| r.id == old.id).unwrap();
        assert_eq!(row.cost, old.cost);
        assert_eq!(row.occurred_at, old.occurred_at);
        assert_eq!(row.tokens.total(), old.tokens.total());
        assert_eq!(row.revision, old.revision + 1);
        assert!(row.tokens.reasoning > 0 && row.timing.is_some());
    }
    assert_eq!(engine.scan(true).unwrap().updated, 0);
    let server = Store::open(&tmp.path().join("server.db")).unwrap();
    let push = PushRequest {
        protocol_version: 4,
        device_id: after[0].device_id.clone(),
        device_name: "fixture".into(),
        records: after.clone(),
    };
    sync::push(&server, push).unwrap();
    let remote = Store::open(&tmp.path().join("remote.db")).unwrap();
    sync::apply_changes(&remote, &sync::changes(&server, 0, 500).unwrap()).unwrap();
    assert_eq!(remote.records("all").unwrap(), after);
    let mut bad = after[0].clone();
    bad.timing.as_mut().unwrap().first_token_ms = Some(1);
    bad.timing.as_mut().unwrap().first_token_source = Some("".into());
    assert!(sync::validate_record(&bad).is_err());
    let logs = engine
        .execute("usage_query", json!({"kind":"logs"}))
        .unwrap();
    for row in logs["data"].as_array().unwrap() {
        assert_eq!(row["timingMethod"], "estimated");
        assert!(row["firstTokenMs"].is_null());
        assert_eq!(row["estimatedSpeed"]["basis"], "end_to_end");
        assert!(row["reasoningTokens"].as_u64().unwrap() > 0);
    }
    drop(engine);
    let engine = Engine::open(&db).unwrap();
    assert_eq!(engine.scan(false).unwrap().updated, 0);
    assert_eq!(engine.lock().unwrap().local_records().unwrap(), after);
    // Invalid optional breakdowns cannot revoke existing completed usage.
    let invalid = MODERN.replace("\"thinking_tokens\":800", "\"thinking_tokens\":1001");
    fs::write(&path, invalid).unwrap();
    let report = engine.scan(false).unwrap();
    assert!(report.deferred_files > 0);
    assert_eq!(engine.lock().unwrap().local_records().unwrap(), after);
}
