use harness_scope_core::{
    harness::{claude::Claude, HarnessAdapter, TimelineIndex},
    model::*,
    pricing,
    store::Store,
    sync, Engine,
};
use serde_json::{json, Value};
use std::{fs, path::Path};

fn engine(p: &Path, harness: &str) -> Engine {
    let e = Engine::open(&p.join("scope.db")).unwrap();
    let s = e.lock().unwrap();
    let mut c = s.config().unwrap();
    c.sources = vec![Source {
        id: "test".into(),
        harness: harness.into(),
        root: p.to_string_lossy().into(),
        enabled: true,
    }];
    c.timezone = "UTC".into();
    c.server_url.clear();
    c.server_token.clear();
    s.set_meta("config", &c).unwrap();
    drop(s);
    e
}
fn event(session: &str, message: &str, request: &str, input: u64, output: u64) -> Value {
    json!({"type":"assistant","timestamp":"2026-10-28T01:00:00Z","sessionId":session,"requestId":request,
        "message":{"id":message,"model":"synthetic-model","usage":{"input_tokens":input,"output_tokens":output}}})
}
fn write(p: &Path, rows: &[Value]) {
    fs::write(p, rows.iter().map(|r| format!("{r}\n")).collect::<String>()).unwrap();
}
fn parse(rows: &[Value]) -> Vec<ParsedUsage> {
    let r = Claude
        .parse(Path::new("test.jsonl"), rows, &TimelineIndex::new())
        .unwrap();
    assert!(r.complete, "{:?}", r.diagnostics);
    r.usage
}
fn scan(e: &Engine) {
    let r = e.scan(false).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
}
fn active(e: &Engine) -> Vec<UsageRecord> {
    e.lock().unwrap().records("local").unwrap()
}
fn old_record(e: &Engine, p: ParsedUsage, session: &str) -> UsageRecord {
    UsageRecord {
        id: format!("claude:{session}:{}", p.native_event_id),
        device_id: e.lock().unwrap().device_id().unwrap(),
        harness: "claude".into(),
        parser_version: "claude-v2-request-timing".into(),
        session_id: Some(session.into()),
        native_event_id: Some(p.native_event_id),
        occurred_at: p.occurred_at,
        model: p.model,
        provider: p.provider,
        tokens: p.tokens,
        cost: Cost {
            amount_usd: Some("7.12345".into()),
            method: "estimated".into(),
            priced_at: 1234,
            ..Cost::default()
        },
        timing: p.timing,
        request_metadata: None,
        grain: "event".into(),
        count: 1,
        record_source: "session".into(),
        revision: 1,
        deleted: false,
        status_code: None,
        legacy_date: None,
        legacy_timezone: None,
    }
}

#[test]
fn v6_upgrade_preserves_payloads_quotes_cursors_then_deduplicates_with_sync_tombstones() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    let e = engine(p, "claude");
    let a = event("a", "msg_same", "req_same", 100, 10);
    let mut b = a.clone();
    b["sessionId"] = json!("b");
    write(&p.join("a.jsonl"), std::slice::from_ref(&a));
    write(&p.join("b.jsonl"), &[b.clone()]);
    let first = old_record(&e, parse(&[a])[0].clone(), "a");
    let mut second = old_record(&e, parse(&[b])[0].clone(), "b");
    second.cost.amount_usd = Some("9".into());
    let mut missing = first.clone();
    missing.id = "claude:missing:msg_missing".into();
    missing.native_event_id = Some("msg_missing".into());
    let before = {
        let s = e.lock().unwrap();
        for (r, path) in [
            (&first, "a.jsonl"),
            (&second, "b.jsonl"),
            (&missing, "removed.jsonl"),
        ] {
            s.upsert(r.clone(), &p.join(path).to_string_lossy())
                .unwrap();
        }
        s.set_meta("remote_cursor", &42).unwrap();
        s.set_meta("test_setting", &json!({"keep":true})).unwrap();
        s.conn.execute_batch("DROP TABLE record_sources;DROP TABLE record_retire_candidates;DROP TABLE claude_event_bindings;PRAGMA user_version=6;").unwrap();
        s.local_records().unwrap()
    };
    drop(e);
    let e = Engine::open(&p.join("scope.db")).unwrap();
    {
        let s = e.lock().unwrap();
        assert_eq!(s.schema_version().unwrap(), 7);
        assert_eq!(s.local_records().unwrap(), before);
        assert_eq!(s.meta::<u64>("remote_cursor").unwrap(), Some(42));
        assert_eq!(
            s.meta::<Value>("test_setting").unwrap(),
            Some(json!({"keep":true}))
        );
        assert_eq!(
            s.conn
                .query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
    }
    scan(&e);
    let rows = active(&e);
    assert_eq!(rows.len(), 2);
    let winner = rows.iter().find(|r| r.id == first.id).unwrap();
    assert_eq!(winner.cost, first.cost);
    assert!(
        e.lock()
            .unwrap()
            .record(&second.id)
            .unwrap()
            .unwrap()
            .deleted
    );
    assert_eq!(
        e.lock().unwrap().record(&second.id).unwrap().unwrap().cost,
        second.cost
    );
    assert_eq!(
        e.lock().unwrap().record(&missing.id).unwrap().unwrap(),
        missing
    );
    let stable = e.lock().unwrap().local_records().unwrap();
    scan(&e);
    assert_eq!(e.lock().unwrap().local_records().unwrap(), stable);
    let server = Store::open(&p.join("server.db")).unwrap();
    let s = e.lock().unwrap();
    let request = PushRequest {
        protocol_version: 4,
        device_id: s.device_id().unwrap(),
        device_name: "test".into(),
        records: s.pending().unwrap(),
    };
    sync::push(&server, request.clone()).unwrap();
    sync::push(&server, request).unwrap();
    let remote = Store::open(&p.join("remote.db")).unwrap();
    sync::apply_changes(&remote, &sync::changes(&server, 0, 500).unwrap()).unwrap();
    assert_eq!(remote.records("all").unwrap().len(), 2);
}

#[test]
fn gateway_requests_and_advisors_have_distinct_stable_identities() {
    let mut b = event("s", "ocgo", "request-2", 300, 20);
    b["timestamp"] = json!("2026-10-28T01:01:00Z");
    let rows = parse(&[event("s", "ocgo", "request-1", 100, 10), b]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows.iter().map(|r| r.tokens.total()).sum::<u64>(), 430);
    let mut v = event("s", "msg_advisor", "req_advisor", 1, 2);
    v["message"]["usage"]["iterations"] = json!([{"type":"message","input_tokens":1,"output_tokens":2},{"type":"advisor_message","model":"advisor-model","input_tokens":10,"output_tokens":2}]);
    let a = parse(&[v.clone(), v]);
    assert_eq!(a.len(), 2);
    assert_eq!(a.iter().map(|r| r.tokens.total()).sum::<u64>(), 15);
    assert!(a.iter().any(|r| r.model == "advisor-model"));
}

#[test]
fn cross_session_copies_are_idempotent_and_unchanged_lmstudio_copy_keeps_events() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    write(
        &p.join("a.jsonl"),
        &[event("a", "msg_same", "req_same", 100, 10)],
    );
    write(
        &p.join("b.jsonl"),
        &[event("b", "msg_same", "req_same", 100, 10)],
    );
    let e = engine(p, "claude");
    scan(&e);
    assert_eq!(active(&e).len(), 1);
    let before = active(&e);
    scan(&e);
    assert_eq!(active(&e), before);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    let mut original: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/lmstudio/1793000000000.conversation.json"
    ))
    .unwrap();
    for name in ["a", "b"] {
        fs::write(
            p.join(format!("{name}.conversation.json")),
            original.to_string(),
        )
        .unwrap();
    }
    let e = engine(p, "lmstudio");
    scan(&e);
    assert_eq!(active(&e).len(), 2);
    original["messages"] = json!([]);
    fs::write(p.join("a.conversation.json"), original.to_string()).unwrap();
    scan(&e);
    assert_eq!(active(&e).len(), 2);
    scan(&e);
    assert_eq!(active(&e).len(), 2);
    fs::write(p.join("b.conversation.json"), "{").unwrap();
    assert!(!e.scan(false).unwrap().errors.is_empty());
    assert_eq!(active(&e).len(), 2);
    fs::write(p.join("b.conversation.json"), original.to_string()).unwrap();
    scan(&e);
    assert!(active(&e).is_empty());
}

fn opencode(p: &Path, boundary: &str) -> rusqlite::Connection {
    let c = rusqlite::Connection::open(p.join("opencode.db")).unwrap();
    c.execute_batch("CREATE TABLE session_v2(id TEXT PRIMARY KEY,title TEXT,directory TEXT,time_created INTEGER,time_updated INTEGER,fork_session_id TEXT,fork_boundary TEXT);CREATE TABLE session_message(id TEXT PRIMARY KEY,session_id TEXT,type TEXT,data TEXT,seq INTEGER);INSERT INTO session_v2 VALUES('parent','parent','/synthetic',1793149200000,1793149201000,NULL,NULL);").unwrap();
    c.execute("INSERT INTO session_v2 VALUES('fork','fork','/synthetic',1793149200000,1793149201000,'parent',?1)",[boundary]).unwrap();
    let data = json!({"role":"assistant","modelID":"model","providerID":"openai","time":{"created":1793149200000_i64,"completed":1793149201000_i64},"tokens":{"input":100,"output":10,"reasoning":0,"cache":{"read":0,"write":0}}});
    for (id, session, seq) in [
        ("original", "parent", 1),
        ("boundary", "parent", 8),
        ("inherited", "fork", 1),
        ("fresh", "fork", 2),
    ] {
        c.execute(
            "INSERT INTO session_message VALUES(?1,?2,'assistant',?3,?4)",
            rusqlite::params![id, session, data.to_string(), seq],
        )
        .unwrap();
    }
    c
}
#[test]
fn opencode_fork_before_uses_sparse_parent_sequence_and_missing_boundary_defers() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    let c = opencode(p, r#"{"type":"before","messageID":"boundary"}"#);
    let e = engine(p, "opencode");
    scan(&e);
    assert_eq!(active(&e).len(), 3);
    c.execute(
        "UPDATE session_v2 SET fork_boundary=?1 WHERE id='fork'",
        [r#"{"type":"through","messageID":"missing"}"#],
    )
    .unwrap();
    assert!(!e.scan(true).unwrap().errors.is_empty());
    assert_eq!(active(&e).len(), 3);
}
#[test]
fn archive_bootstrap_skips_normal_refresh_but_confirmed_reimport_reads_new_archives() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    let bytes = include_bytes!("../../../tests/fixtures/codex/session.jsonl");
    fs::write(
        p.join("session.jsonl.zst"),
        zstd::stream::encode_all(&bytes[..], 1).unwrap(),
    )
    .unwrap();
    let e = engine(p, "codex");
    scan(&e);
    let before = active(&e);
    assert!(!before.is_empty());
    fs::write(p.join("bad.json.zst"), "bad zstd").unwrap();
    scan(&e);
    assert!(e.scan(true).unwrap().errors.is_empty());
    assert!(e
        .execute("reimport_harness", json!({"harness":"codex"}))
        .is_err());
    let result = e
        .execute(
            "reimport_harness",
            json!({"harness":"codex","confirmed":true}),
        )
        .unwrap();
    assert!(!result["errors"].as_array().unwrap().is_empty());
    assert_eq!(active(&e), before);
    fs::write(
        p.join("bad.json.zst"),
        zstd::stream::encode_all(&bytes[..], 1).unwrap(),
    )
    .unwrap();
    assert!(e
        .execute(
            "reimport_harness",
            json!({"harness":"codex","confirmed":true})
        )
        .unwrap()["errors"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(active(&e), before);
}
#[test]
fn cost_coverage_is_carried_through_every_aggregation_and_averages_exclude_unknowns() {
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path(), "claude");
    write(
        &dir.path().join("s.jsonl"),
        &[
            event("s", "msg_a", "req_a", 100, 10),
            event("s", "msg_b", "req_b", 200, 20),
        ],
    );
    scan(&e);
    {
        let s = e.lock().unwrap();
        let mut r = s.records("local").unwrap().remove(0);
        r.cost.amount_usd = Some("2".into());
        r.cost.method = "reported".into();
        s.upsert(r, "test").unwrap();
    }
    for kind in ["summary", "models", "providers", "sources", "trends"] {
        let v = e
            .execute(
                "usage_query",
                json!({"kind":kind,"startDate":1793149200_i64,"endDate":1793152799_i64}),
            )
            .unwrap();
        let row = if kind == "summary" { &v } else { &v[0] };
        assert_eq!(row["unpricedCount"], 1, "{kind}: {v}");
        assert_eq!(row["pricedCount"], 1);
        assert!(row["unpricedTokens"].as_u64().unwrap() > 0);
        if kind == "models" {
            assert_eq!(row["avgCostPerRequest"], "2");
        }
    }
}
#[test]
fn provider_tariffs_use_absolute_rates_context_cache_and_keep_frozen_quotes() {
    let mut v = event("s", "msg_speed", "req_speed", 1000, 100);
    v["message"]["usage"]["speed"] = json!("fast");
    let mut p = parse(&[v])[0].clone();
    p.provider = Some("custom".into());
    assert_eq!(p.request_metadata.as_ref().unwrap().speed(), Some("fast"));
    let price:Price=serde_json::from_value(json!({"modelId":"synthetic-model","displayName":"test","provider":"custom","inputCostPerMillion":"4","outputCostPerMillion":"20","cacheReadCostPerMillion":"1","cacheCreationCostPerMillion":"5","source":"manual","sourceKey":null,"fetchedAt":0,
        "requestTariff":{"version":"test-v1","tiers":[{"id":"fast","speed":"fast","abovePromptTokens":0,"inputCostPerMillion":"8","outputCostPerMillion":"40","cacheReadCostPerMillion":"2","cacheCreationCostPerMillion":"10"},{"id":"fast-long","speed":"fast","abovePromptTokens":1000,"inputCostPerMillion":"12","outputCostPerMillion":"60","cacheReadCostPerMillion":"3","cacheCreationCostPerMillion":"15"}]}})).unwrap();
    let c = pricing::resolve(&p, None, std::slice::from_ref(&price)).unwrap();
    assert_eq!(c.amount_usd.as_deref(), Some("0.012"));
    assert_eq!(
        c.snapshot
            .as_ref()
            .unwrap()
            .request_tariff
            .as_ref()
            .unwrap()
            .selected_tier
            .as_deref(),
        Some("fast")
    );
    p.tokens.cache_read = 1;
    let long = pricing::resolve(&p, None, std::slice::from_ref(&price)).unwrap();
    assert_eq!(long.amount_usd.as_deref(), Some("0.018003"));
    let dir = tempfile::tempdir().unwrap();
    let e = engine(dir.path(), "claude");
    let mut old = old_record(&e, p.clone(), "s");
    old.cost = long.clone();
    let mut changed = price.clone();
    changed.input_cost_per_million = "99".into();
    changed.request_tariff.as_mut().unwrap().tiers[1].input_cost_per_million = "99".into();
    assert_eq!(pricing::resolve(&p, Some(&old), &[changed]).unwrap(), long);
    p.provider = Some("other".into());
    assert!(pricing::resolve(&p, None, std::slice::from_ref(&price))
        .unwrap()
        .amount_usd
        .is_none());
    let mut invalid = price;
    invalid.provider = None;
    assert!(pricing::validate(&invalid).is_err());
}

#[test]
fn failed_v6_migration_rolls_back_schema_and_is_retryable() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("v6.db");
    let c = rusqlite::Connection::open(&p).unwrap();
    c.execute_batch(include_str!("../src/schema.sql")).unwrap();
    c.execute_batch("PRAGMA user_version=6; INSERT INTO records VALUES('device','broken',1,'not-json','missing');").unwrap();
    assert!(Store::open(&p).is_err());
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        6
    );
    assert!(!c
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='record_sources')",
            [],
            |r| r.get::<_, bool>(0)
        )
        .unwrap());
    assert_eq!(
        c.query_row("SELECT payload FROM records", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "not-json"
    );
    // Repair the synthetic invalid row; migration can be retried without reset.
    c.execute(
        "UPDATE records SET payload=?1",
        [json!({"deleted":false,"recordSource":"session"}).to_string()],
    )
    .unwrap();
    let s = Store::open(&p).unwrap();
    assert_eq!(s.schema_version().unwrap(), 7);
    assert_eq!(
        s.conn
            .query_row("SELECT COUNT(*) FROM record_sources", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn v7_source_reconciliation_survives_restart_and_does_not_delete_other_harness_history() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    let e = engine(p, "claude");
    let event = event("s", "msg_s", "req_s", 10, 1);
    write(&p.join("a.jsonl"), std::slice::from_ref(&event));
    write(&p.join("b.jsonl"), &[event]);
    scan(&e);
    let before = active(&e);
    drop(e);
    fs::write(p.join("a.jsonl"), "").unwrap();
    let e = Engine::open(&p.join("scope.db")).unwrap();
    scan(&e);
    assert_eq!(active(&e), before);
    let report = e
        .execute(
            "reimport_harness",
            json!({"harness":"claude","confirmed":true}),
        )
        .unwrap();
    assert!(report["errors"].as_array().unwrap().is_empty());
    assert_eq!(active(&e), before);
}
