use harness_scope_core::{model::*, query, store::Store, Engine};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

const AT: i64 = 1792339200;
fn record(device: &str, id: &str, at: i64) -> UsageRecord {
    UsageRecord {
        device_id: device.into(),
        id: id.into(),
        provider: Some("route".into()),
        harness: "codex".into(),
        parser_version: "fixture".into(),
        session_id: Some("session".into()),
        native_event_id: Some(id.into()),
        occurred_at: at,
        model: "raw-model-high".into(),
        grain: "event".into(),
        count: 1,
        tokens: Tokens {
            input: 100,
            output: 300,
            cache_read: 50,
            ..Default::default()
        },
        cost: Cost {
            amount_usd: Some("0.1234567890123456789012345678".into()),
            method: "reported".into(),
            ..Default::default()
        },
        record_source: "session".into(),
        revision: 1,
        deleted: false,
        status_code: Some(200),
        legacy_date: None,
        legacy_timezone: None,
    }
}
fn q(e: &Engine, kind: &str, args: Value) -> Value {
    let mut args = args;
    args["kind"] = json!(kind);
    e.execute("usage_query", args).unwrap()
}

#[test]
fn version_one_upgrade_preserves_facts_outbox_and_exact_costs_and_restarts_once() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("v1.db");
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch("CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT NOT NULL); CREATE TABLE records(device_id TEXT NOT NULL,id TEXT NOT NULL,revision INTEGER NOT NULL,payload TEXT NOT NULL,source_path TEXT NOT NULL,PRIMARY KEY(device_id,id)); CREATE TABLE outbox(id TEXT PRIMARY KEY,revision INTEGER NOT NULL,payload TEXT NOT NULL); PRAGMA user_version=1;").unwrap();
    c.execute("INSERT INTO meta VALUES('device_id',?1)", ["\"device\""])
        .unwrap();
    let a = record("device", "one", AT);
    let text = serde_json::to_string(&a).unwrap();
    c.execute(
        "INSERT INTO records VALUES('device','one',1,?1,'original-path')",
        [&text],
    )
    .unwrap();
    c.execute("INSERT INTO outbox VALUES('one',1,?1)", [&text])
        .unwrap();
    drop(c);
    let e = Engine::open(&path).unwrap();
    assert_eq!(e.lock().unwrap().local_records().unwrap(), vec![a.clone()]);
    assert_eq!(e.lock().unwrap().pending().unwrap(), vec![a]);
    let summary = q(&e, "summary", json!({}));
    assert_eq!(summary["totalCost"], "0.1234567890123456789012345678");
    assert_eq!(q(&e, "logs", json!({}))["total"], 1);
    let version = e.lock().unwrap().usage_version().unwrap();
    drop(e);
    let e = Engine::open(&path).unwrap();
    assert_eq!(e.lock().unwrap().usage_version().unwrap(), version);
    assert_eq!(q(&e, "summary", json!({})), summary);
    let s = e.lock().unwrap();
    assert_eq!(
        s.conn
            .query_row("SELECT payload FROM records", [], |r| r.get::<_, String>(0))
            .unwrap(),
        text
    );
    assert_eq!(
        s.conn
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn writes_deletes_remote_resets_and_rollbacks_update_the_index_atomically() {
    let tmp = tempfile::tempdir().unwrap();
    let e = Engine::open(&tmp.path().join("scope.db")).unwrap();
    let device = e.lock().unwrap().device_id().unwrap();
    let mut r = record(&device, "one", AT);
    e.lock().unwrap().upsert(r.clone(), "source").unwrap();
    assert_eq!(q(&e, "summary", json!({}))["totalRequests"], 1);
    {
        let s = e.lock().unwrap();
        let version = s.usage_version().unwrap();
        assert!(!s.upsert(r.clone(), "source").unwrap());
        s.set_meta("last_scan_at", &123).unwrap();
        assert_eq!(s.usage_version().unwrap(), version);
        let failure: anyhow::Result<()> = s.transaction(|| {
            s.upsert(record(&device, "rolled-back", AT), "source")?;
            anyhow::bail!("rollback")
        });
        assert!(failure.is_err());
        assert_eq!(s.usage_version().unwrap(), version);
    }
    assert_eq!(q(&e, "logs", json!({}))["total"], 1);
    r.provider = Some("new-route".into());
    r.model = "Original-Model-Text".into();
    r.tokens.input = 250;
    e.lock().unwrap().upsert(r.clone(), "source").unwrap();
    assert_eq!(
        q(&e, "summary", json!({"providerName":"route"}))["totalRequests"],
        0
    );
    assert_eq!(
        q(
            &e,
            "summary",
            json!({"providerName":"new-route","model":"Original-Model-Text"})
        )["totalInputTokens"],
        250
    );
    {
        let s = e.lock().unwrap();
        for device in [&device, "remote"] {
            let remote = record(device, "one", AT);
            s.conn
                .execute(
                    "INSERT INTO remote_records VALUES(?1,'one',1,?2)",
                    rusqlite::params![device, serde_json::to_string(&remote).unwrap()],
                )
                .unwrap();
        }
    }
    assert_eq!(q(&e, "summary", json!({"scope":"all"}))["totalRequests"], 2);
    assert_eq!(
        q(&e, "summary", json!({"scope":"remote"}))["totalRequests"],
        1
    );
    assert_eq!(
        q(&e, "summary", json!({"scope":device}))["totalRequests"],
        1
    );
    e.lock()
        .unwrap()
        .conn
        .execute("DELETE FROM remote_records", [])
        .unwrap();
    assert_eq!(q(&e, "summary", json!({"scope":"all"}))["totalRequests"], 1);
    r.deleted = true;
    e.lock().unwrap().upsert(r, "source").unwrap();
    assert_eq!(q(&e, "logs", json!({}))["total"], 0);
    assert_eq!(q(&e, "hasUsage", json!({})), false);
    e.lock()
        .unwrap()
        .conn
        .execute("DELETE FROM records", [])
        .unwrap();
    assert_eq!(
        e.lock()
            .unwrap()
            .conn
            .query_row("SELECT COUNT(*) FROM usage_index", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn pagination_does_not_decode_unselected_snapshots_or_unrelated_timings() {
    let tmp = tempfile::tempdir().unwrap();
    let e = Engine::open(&tmp.path().join("scope.db")).unwrap();
    let device = e.lock().unwrap().device_id().unwrap();
    {
        let s = e.lock().unwrap();
        for id in ["a", "b", "c"] {
            s.upsert(record(&device, id, AT), "source").unwrap();
        }
        s.upsert(record(&device, "old", AT - 1), "source").unwrap();
        s.conn.execute("UPDATE records SET payload=json_set(payload,'$.cost.snapshot',json('{\"unreadable\":true}')) WHERE id='old'",[]).unwrap();
        s.conn
            .execute(
                "INSERT INTO record_timings VALUES(?1,'old','invalid json')",
                [&device],
            )
            .unwrap();
    }
    let args = json!({"startDate":AT,"endDate":AT,"pageSize":2});
    let first = q(&e, "logs", args.clone());
    assert_eq!(first["total"], 3);
    assert_eq!(first["data"][0]["requestId"], format!("{device}|a"));
    assert_eq!(first["data"][1]["requestId"], format!("{device}|b"));
    let mut next = args.clone();
    next["page"] = json!(1);
    assert_eq!(
        q(&e, "logs", next)["data"][0]["requestId"],
        format!("{device}|c")
    );
    assert_eq!(q(&e, "summary", json!({}))["totalRequests"], 4);
    assert_eq!(q(&e, "firstDate", json!({})), "2026-10-18");
    assert_eq!(q(&e, "logs", json!({"page":u64::MAX}))["data"], json!([]));
}

#[test]
fn readers_serve_committed_data_while_the_scan_connection_holds_a_write_transaction() {
    let tmp = tempfile::tempdir().unwrap();
    let e = Arc::new(Engine::open(&tmp.path().join("scope.db")).unwrap());
    let device = e.lock().unwrap().device_id().unwrap();
    let r = record(&device, "one", AT);
    e.lock().unwrap().upsert(r.clone(), "source").unwrap();
    let s = e.lock().unwrap();
    s.conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut changed = r;
    changed.tokens.input = 500;
    s.upsert(changed, "source").unwrap();
    let (send, recv) = std::sync::mpsc::channel();
    let reader = e.clone();
    let thread = std::thread::spawn(move || send.send(q(&reader, "summary", json!({}))).unwrap());
    let result = recv.recv_timeout(Duration::from_secs(2));
    s.conn.execute_batch("COMMIT").unwrap();
    drop(s);
    thread.join().unwrap();
    assert_eq!(result.unwrap()["totalInputTokens"], 100);
    assert_eq!(q(&e, "summary", json!({}))["totalInputTokens"], 500);
}

#[test]
fn timing_changes_invalidate_usage_but_reads_and_noop_timing_saves_do_not() {
    let tmp = tempfile::tempdir().unwrap();
    let e = Engine::open(&tmp.path().join("scope.db")).unwrap();
    let device = e.lock().unwrap().device_id().unwrap();
    e.lock()
        .unwrap()
        .upsert(record(&device, "one", AT), "source")
        .unwrap();
    let timing = UsageTiming::estimate(
        AT * 1000,
        AT * 1000 + 3000,
        Some(AT * 1000 + 200),
        "test",
        "test",
    )
    .unwrap();
    let version = e.lock().unwrap().usage_version().unwrap();
    e.lock().unwrap().save_timing("one", Some(&timing)).unwrap();
    let after = e.lock().unwrap().usage_version().unwrap();
    assert!(after > version);
    assert_eq!(q(&e, "logs", json!({}))["data"][0]["firstTokenMs"], 200);
    e.lock().unwrap().save_timing("one", Some(&timing)).unwrap();
    assert_eq!(e.lock().unwrap().usage_version().unwrap(), after);
    let s = e.lock().unwrap();
    assert_eq!(
        query::summary(&s.local_records().unwrap())["totalInputTokens"],
        100
    );
    assert_eq!(s.usage_version().unwrap(), after);
}

#[test]
fn failed_projection_migration_does_not_advance_schema_or_modify_facts() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("invalid-v1.db");
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch("CREATE TABLE records(device_id TEXT,id TEXT,revision INTEGER,payload TEXT,source_path TEXT,PRIMARY KEY(device_id,id));INSERT INTO records VALUES('d','i',1,'{}','s');PRAGMA user_version=1;").unwrap();
    drop(c);
    assert!(Store::open(&path).is_err());
    let c = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        c.query_row("SELECT payload FROM records", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "{}"
    );
}

#[test]
fn pricing_reports_actual_usage_changes_and_does_not_invalidate_unchanged_history() {
    let tmp = tempfile::tempdir().unwrap();
    let e = Engine::open(&tmp.path().join("scope.db")).unwrap();
    let device = e.lock().unwrap().device_id().unwrap();
    let mut r = record(&device, "unknown", AT);
    r.cost = Cost::default();
    e.lock().unwrap().upsert(r, "source").unwrap();
    let args = json!({"entries":[{"provider":"route","modelId":"raw-model-high","displayName":"Fixture","inputCostPerMillion":"1","outputCostPerMillion":"2","cacheReadCostPerMillion":"0.1","cacheCreationCostPerMillion":"0","source":"manual"}]});
    assert_eq!(
        e.execute("save_prices_detailed", args.clone()).unwrap(),
        json!({"saved":1,"usageChanged":1})
    );
    let version = e.lock().unwrap().usage_version().unwrap();
    let before = e.lock().unwrap().local_records().unwrap();
    assert_eq!(
        e.execute("save_prices_detailed", args).unwrap(),
        json!({"saved":1,"usageChanged":0})
    );
    assert_eq!(e.lock().unwrap().usage_version().unwrap(), version);
    assert_eq!(e.lock().unwrap().local_records().unwrap(), before);
}

#[test]
fn changing_the_collection_interval_wakes_the_single_background_collector() {
    use harness_scope_core::harness::{
        claude::Claude, Descriptor, HarnessAdapter, ParseResult, Registry, TimelineIndex,
    };
    use std::path::{Path, PathBuf};
    struct Signal(std::sync::mpsc::Sender<()>);
    impl HarnessAdapter for Signal {
        fn descriptor(&self) -> Descriptor {
            Claude.descriptor()
        }
        fn discover(&self, _: &Path) -> anyhow::Result<Vec<PathBuf>> {
            let _ = self.0.send(());
            Ok(vec![])
        }
        fn parse(&self, _: &Path, _: &[Value], _: &TimelineIndex) -> anyhow::Result<ParseResult> {
            unreachable!()
        }
        fn session(&self, _: &Path, _: &[Value]) -> Option<Session> {
            None
        }
        fn messages(&self, _: &[Value]) -> Vec<Value> {
            vec![]
        }
    }
    let tmp = tempfile::tempdir().unwrap();
    let mut e = Engine::open(&tmp.path().join("scope.db")).unwrap();
    let (send, recv) = std::sync::mpsc::channel();
    e.registry = Registry::new(vec![Arc::new(Signal(send))]);
    let mut config = e.lock().unwrap().config().unwrap();
    config.sources = vec![Source {
        id: "signal".into(),
        harness: "claude".into(),
        root: tmp.path().to_string_lossy().into(),
        enabled: true,
    }];
    config.scan_interval_seconds = 60;
    e.lock().unwrap().set_meta("config", &config).unwrap();
    let e = Arc::new(e);
    e.start_background();
    recv.recv_timeout(Duration::from_secs(2)).unwrap();
    config.theme = "dark".into();
    e.execute("save_config", json!({"config":config})).unwrap();
    assert!(matches!(
        recv.recv_timeout(Duration::from_millis(50)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));
    config.scan_interval_seconds = 5;
    e.execute("save_config", json!({"config":config})).unwrap();
    recv.recv_timeout(Duration::from_secs(2)).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while Arc::strong_count(&e) > 1 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
}
