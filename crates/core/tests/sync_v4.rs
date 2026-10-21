use harness_scope_core::{model::*, query, store::Store, sync};
use rusqlite::params;
use serde_json::json;

fn record(device: &str, id: &str) -> UsageRecord {
    serde_json::from_value(json!({"deviceId":device,"id":id,"harness":"codex","parserVersion":"fixture","sessionId":"session","nativeEventId":id,"occurredAt":1792500000,"model":"model","grain":"event","count":1,"tokens":{"input":100,"output":300,"cacheRead":0,"cacheWrite":0,"cacheWrite1h":0,"reasoning":0},"cost":Cost::default(),"recordSource":"session","revision":1,"deleted":false,"statusCode":null,"legacyDate":null,"legacyTimezone":null})).unwrap()
}
fn timing() -> UsageTiming {
    UsageTiming::estimate(
        1000,
        5000,
        Some(2000),
        "codex_token_count",
        "codex_item_started_at_ms",
    )
    .unwrap()
}

#[test]
fn deferred_claude_revisions_cannot_starve_compatible_uploads() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(&dir.path().join("scope.db")).unwrap();
    let device = s.device_id().unwrap();
    for n in 0..201 {
        let mut r = record(&device, &format!("claude:{n:03}"));
        r.harness = "claude".into();
        r.timing = Some(UsageTiming {
            duration_ms: 2000,
            first_token_ms: None,
            duration_source: "claude_api_request_blocks".into(),
            first_token_source: None,
        });
        s.upsert(r, "synthetic").unwrap();
    }
    s.upsert(record(&device, "codex:compatible"), "synthetic")
        .unwrap();
    assert_eq!(s.pending().unwrap().len(), 200);
    assert_eq!(s.pending_claude_metadata_count().unwrap(), 201);
    let compatible = s.pending_compatible(false).unwrap();
    assert_eq!(compatible.len(), 1);
    assert_eq!(compatible[0].id, "codex:compatible");
    assert_eq!(s.pending_count().unwrap(), 202);
    assert_eq!(s.pending_compatible(true).unwrap().len(), 200);
}
fn push(server: &Store, records: Vec<UsageRecord>, version: u32) -> anyhow::Result<PushResponse> {
    sync::push(
        server,
        PushRequest {
            protocol_version: version,
            device_id: records[0].device_id.clone(),
            device_name: "fixture".into(),
            records,
        },
    )
}

#[test]
fn v3_backfill_preserves_facts_identity_and_pending_revisions_and_runs_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v3.db");
    let old;
    {
        let s = Store::open(&path).unwrap();
        old = record(&s.device_id().unwrap(), "historical");
        s.upsert(old.clone(), "original-source").unwrap();
        s.save_timing(&old.id, Some(&timing())).unwrap();
        // Same native ID on another device cannot supply the backfill.
        s.conn
            .execute(
                "INSERT INTO record_timings VALUES('other','missing',?1)",
                [serde_json::to_string(&timing()).unwrap()],
            )
            .unwrap();
        s.upsert(record(&old.device_id, "missing"), "original-source")
            .unwrap();
        s.conn
            .execute_batch("DELETE FROM outbox; PRAGMA user_version=3;")
            .unwrap();
    }
    let s = Store::open(&path).unwrap();
    let upgraded = s.record(&old.id).unwrap().unwrap();
    let mut expected = old.clone();
    expected.timing = Some(timing());
    expected.revision += 1;
    assert_eq!(upgraded, expected);
    assert!(s.record("missing").unwrap().unwrap().timing.is_none());
    assert_eq!(s.pending().unwrap(), vec![expected.clone()]);
    s.ack(&[Ack {
        id: old.id.clone(),
        revision: old.revision,
    }])
    .unwrap();
    assert_eq!(s.pending_count().unwrap(), 1);
    let source: String = s
        .conn
        .query_row(
            "SELECT source_path FROM records WHERE id='historical'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(source, "original-source");
    drop(s);
    let s = Store::open(&path).unwrap();
    assert_eq!(s.record(&old.id).unwrap().unwrap(), expected);
    assert_eq!(s.pending_count().unwrap(), 1);
}

#[test]
fn legacy_change_migration_preserves_cursor_and_reclaims_duplicate_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("server.db");
    let s = Store::open(&path).unwrap();
    let a = record("source", "a");
    let mut b = record("source", "b");
    push(&s, vec![a.clone(), b.clone()], 4).unwrap();
    b.revision = 2;
    push(&s, vec![b.clone()], 4).unwrap();
    s.conn.execute_batch("DROP TABLE server_changes; CREATE TABLE server_changes(seq INTEGER PRIMARY KEY AUTOINCREMENT,device_id TEXT NOT NULL,id TEXT NOT NULL,payload TEXT NOT NULL);").unwrap();
    for (seq, r) in [(1, &a), (2, &b), (3, &b)] {
        s.conn
            .execute(
                "INSERT INTO server_changes VALUES(?1,?2,?3,?4)",
                params![seq, r.device_id, r.id, serde_json::to_string(r).unwrap()],
            )
            .unwrap();
    }
    s.conn.execute_batch("UPDATE sqlite_sequence SET seq=100 WHERE name='server_changes'; PRAGMA user_version=3;").unwrap();
    let instance = s.meta::<String>("server_instance_id").unwrap();
    drop(s);
    let s = Store::open(&path).unwrap();
    assert_eq!(s.meta::<String>("server_instance_id").unwrap(), instance);
    let changes = sync::changes(&s, 1, 1).unwrap();
    assert_eq!(changes.changes[0].record, b);
    assert_eq!(changes.next_cursor, 3);
    assert_eq!(sync::changes(&s, 0, 500).unwrap().changes.len(), 2);
    let mut updated = a;
    updated.revision = 2;
    updated.deleted = true;
    push(&s, vec![updated.clone()], 4).unwrap();
    let changes = sync::changes(&s, 100, 500).unwrap();
    assert_eq!(changes.changes[0].record, updated);
    assert!(changes.next_cursor > 100);
    assert_eq!(sync::changes(&s, 0, 500).unwrap().changes.len(), 2);
    let columns: Vec<String> = s
        .conn
        .prepare("PRAGMA table_info(server_changes)")
        .unwrap()
        .query_map([], |r| r.get(1))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(!columns.iter().any(|v| v == "payload"));
}

#[test]
fn coalescing_preserves_paginated_offline_and_new_device_convergence() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(&dir.path().join("server.db")).unwrap();
    let mut a = record("source", "a");
    let mut b = record("source", "b");
    push(&s, vec![a.clone(), b.clone()], 4).unwrap();
    let client = Store::open(&dir.path().join("client.db")).unwrap();
    let page = sync::changes(&s, 0, 1).unwrap();
    assert!(page.has_more);
    sync::apply_changes(&client, &page).unwrap();
    a.revision = 2;
    a.timing = Some(timing());
    push(&s, vec![a.clone()], 4).unwrap();
    let page = sync::changes(&s, page.next_cursor, 1).unwrap();
    sync::apply_changes(&client, &page).unwrap();
    b.revision = 2;
    b.deleted = true;
    push(&s, vec![b.clone()], 4).unwrap();
    let mut cursor = page.next_cursor;
    loop {
        let page = sync::changes(&s, cursor, 1).unwrap();
        sync::apply_changes(&client, &page).unwrap();
        cursor = page.next_cursor;
        if !page.has_more {
            break;
        }
    }
    assert_eq!(client.records("all").unwrap(), vec![a.clone()]);
    let fresh = Store::open(&dir.path().join("fresh.db")).unwrap();
    sync::apply_changes(&fresh, &sync::changes(&s, 0, 500).unwrap()).unwrap();
    assert_eq!(fresh.records("all").unwrap(), vec![a.clone()]);
    let detail = query::query(
        &fresh,
        "detail",
        &json!({"scope":"all","requestId":"source|a"}),
    )
    .unwrap();
    assert_eq!(detail["durationMs"], 4000);
    assert_eq!(detail["firstTokenMs"], 1000);
    let stats = query::query(&fresh, "models", &json!({"scope":"all"})).unwrap();
    assert_eq!(stats[0]["timingSampleCount"], 1);
    // A delayed lower revision must not erase timing, even if delivered at a later cursor.
    let mut stale = a.clone();
    stale.revision = 1;
    stale.timing = None;
    let mut response = sync::changes(&s, 0, 500).unwrap();
    response.changes = vec![Change {
        seq: 1000,
        record: stale,
    }];
    sync::apply_changes(&fresh, &response).unwrap();
    assert_eq!(
        fresh.timings().unwrap().get(&("source".into(), "a".into())),
        Some(&timing())
    );
    let count: i64 = s
        .conn
        .query_row("SELECT COUNT(*) FROM server_changes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2);
    push(&s, vec![a], 4).unwrap();
    assert_eq!(sync::changes(&s, cursor, 500).unwrap().changes.len(), 0);
}

#[test]
fn timing_rejects_invalid_values_unknown_sources_and_legacy_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(&dir.path().join("s.db")).unwrap();
    let mut r = record("source", "a");
    r.timing = Some(timing());
    assert!(push(&s, vec![r.clone()], 3).is_err());
    push(&s, vec![r.clone()], 4).unwrap();
    let mut downgrade = r.clone();
    downgrade.timing = None;
    downgrade.revision += 1;
    assert!(push(&s, vec![downgrade], 3).is_err());
    for bad in [-1, 0, 99, 3_600_001] {
        let mut bad_record = r.clone();
        bad_record.timing.as_mut().unwrap().duration_ms = bad;
        assert!(sync::validate_record(&bad_record).is_err());
    }
    let mut bad = r.clone();
    bad.timing.as_mut().unwrap().first_token_ms = Some(4001);
    assert!(sync::validate_record(&bad).is_err());
    let mut bad = r.clone();
    bad.timing.as_mut().unwrap().duration_source = "private path".into();
    assert!(sync::validate_record(&bad).is_err());
    let mut wire = serde_json::to_value(r).unwrap();
    wire["timing"]["transcript"] = json!("private");
    assert!(serde_json::from_value::<UsageRecord>(wire).is_err());
}

#[test]
fn invalid_backfill_rolls_back_schema_and_payload_changes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v3.db");
    let s = Store::open(&path).unwrap();
    let r = record(&s.device_id().unwrap(), "a");
    s.upsert(r.clone(), "source").unwrap();
    let mut invalid = timing();
    invalid.duration_ms = -1;
    s.save_timing("a", Some(&invalid)).unwrap();
    s.conn.execute_batch("PRAGMA user_version=3").unwrap();
    drop(s);
    assert!(Store::open(&path).is_err());
    let conn = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 3);
    let payload: String = conn
        .query_row("SELECT payload FROM records", [], |r| r.get(0))
        .unwrap();
    assert_eq!(serde_json::from_str::<UsageRecord>(&payload).unwrap(), r);
}
