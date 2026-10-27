use harness_scope_core::{
    harness::{cherry::Cherry, HarnessAdapter, TimelineIndex},
    model::{PushRequest, Source},
    pricing,
    store::Store,
    sync, Engine,
};
use rusqlite::Connection;
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

fn seed(path: &Path) -> Connection {
    let db = Connection::open(path).unwrap();
    db.execute_batch(r#"
        PRAGMA journal_mode=WAL;
        CREATE TABLE topic(id TEXT PRIMARY KEY,name TEXT,created_at INTEGER,last_activity_at INTEGER,deleted_at INTEGER);
        CREATE TABLE agent_session(id TEXT PRIMARY KEY,name TEXT,created_at INTEGER,last_activity_at INTEGER);
        CREATE TABLE message(id TEXT PRIMARY KEY,topic_id TEXT,role TEXT,data TEXT,created_at INTEGER,deleted_at INTEGER);
        CREATE TABLE agent_session_message(id TEXT PRIMARY KEY,session_id TEXT,role TEXT,data TEXT,created_at INTEGER);
        CREATE TABLE ai_usage_record(id TEXT PRIMARY KEY,request_id TEXT,record_kind TEXT,request_count INTEGER,message_kind TEXT,message_id TEXT,model_id TEXT,provider_id TEXT,input_tokens INTEGER,output_tokens INTEGER,total_tokens INTEGER,no_cache_tokens INTEGER,cache_read_tokens INTEGER,cache_write_tokens INTEGER,reasoning_tokens INTEGER,cost REAL,cost_currency TEXT,cost_source TEXT,created_at INTEGER,modality TEXT);
        INSERT INTO topic VALUES('topic','Chat',1793000000000,1793000005000,NULL);
        INSERT INTO agent_session VALUES('agent','Agent',1793000000000,1793000005000);
        INSERT INTO message VALUES('msg','topic','assistant','{"parts":[{"type":"reasoning","text":"thought"},{"type":"text","text":"answer"}]}',1793000000000,NULL);
        INSERT INTO agent_session_message VALUES('agent-msg','agent','user','{"parts":[{"type":"text","text":"hello"}]}',1793000001000);
        INSERT INTO ai_usage_record VALUES('one','request-one','invocation',1,'chat','msg','model','provider',100,30,130,75,25,NULL,10,0.001,'USD','provider',1793000000000,'language');
        INSERT INTO ai_usage_record VALUES('two','request-two','invocation',1,'agent-session','agent-msg','model','provider',40,10,50,40,0,NULL,0,0.5,'USD','computed',1793000001000,'language');
        INSERT INTO ai_usage_record VALUES('legacy','legacy-request','legacy-aggregate',1,'chat','msg','model','provider',100,40,140,NULL,NULL,NULL,NULL,NULL,NULL,NULL,1793000002000,'language');
        INSERT INTO ai_usage_record VALUES('image','image-request','invocation',1,'chat','msg','model','provider',NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,1793000003000,'image');
    "#).unwrap();
    db
}

#[test]
fn wal_sessions_usage_and_incomplete_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cherrystudio.sqlite");
    let db = seed(&path);
    let initial_changes = db.total_changes();
    let sessions = Cherry.read_metadata(&path).unwrap();
    assert_eq!(sessions.len(), 2);
    let messages = Cherry.read_messages(&path, Some("chat:topic")).unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["blocks"][0]["type"], "thinking");
    assert_eq!(messages[0]["blocks"][1]["text"], "answer");
    assert_eq!(
        Cherry.read_messages(&path, Some("agent:agent")).unwrap()[0]["content"],
        "hello"
    );
    let (rows, _, _, complete) = Cherry.read_usage_source(&path).unwrap();
    assert!(complete);
    assert!(!serde_json::to_string(&rows).unwrap().contains("answer"));
    let parsed = Cherry.parse(&path, &rows, &TimelineIndex::new()).unwrap();
    assert!(parsed.complete && parsed.diagnostics.is_empty());
    assert_eq!(parsed.usage.len(), 2);
    let first = parsed
        .usage
        .iter()
        .find(|x| x.native_event_id == "one")
        .unwrap();
    assert_eq!(
        (
            first.tokens.input,
            first.tokens.cache_read,
            first.tokens.output,
            first.tokens.reasoning
        ),
        (75, 25, 30, 10)
    );
    assert_eq!(first.session_id, "chat:topic");
    assert_eq!(first.reported_cost.as_deref(), Some("0.001"));
    assert!(parsed
        .usage
        .iter()
        .find(|x| x.native_event_id == "two")
        .unwrap()
        .reported_cost
        .is_none());
    assert_eq!(db.total_changes(), initial_changes);

    let engine = Engine::open(&dir.path().join("scope.db")).unwrap();
    let store = engine.lock().unwrap();
    let mut config = store.config().unwrap();
    config.sources = vec![Source {
        id: "cherry-fixture".into(),
        harness: "cherry".into(),
        root: path.to_string_lossy().into(),
        enabled: true,
    }];
    store.set_meta("config", &config).unwrap();
    drop(store);
    let scan = engine.scan(false).unwrap();
    assert_eq!(scan.imported, 2);
    assert_eq!(scan.deferred_files, 0);
    assert!(scan.errors.is_empty());
    let records = engine.lock().unwrap().records("local").unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(
        records
            .iter()
            .find(|r| r.id == "cherry:one")
            .unwrap()
            .cost
            .amount_usd
            .as_deref(),
        Some("0.001")
    );
    assert!(records
        .iter()
        .find(|r| r.id == "cherry:two")
        .unwrap()
        .cost
        .amount_usd
        .is_none());
    let summary = engine
        .execute("usage_query", json!({"kind":"summary","appType":"cherry"}))
        .unwrap();
    assert_eq!(summary["totalRequests"], 2);
    assert_eq!(summary["realTotalTokens"], 180);
    let apps = engine
        .execute("usage_query", json!({"kind":"summaryByApp"}))
        .unwrap();
    assert!(apps
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["appType"] == "cherry" && v["summary"]["totalRequests"] == 2));
    assert_eq!(engine.sessions().unwrap().len(), 2);
    db.execute("UPDATE topic SET name='Updated' WHERE id='topic'", [])
        .unwrap();
    assert_eq!(
        engine
            .sessions()
            .unwrap()
            .iter()
            .find(|s| s.session_id == "chat:topic")
            .unwrap()
            .title
            .as_deref(),
        Some("Updated")
    );

    // Real synchronization serialization uses the same existing wire contract.
    let server = Store::open(&dir.path().join("server.db")).unwrap();
    let remote = Store::open(&dir.path().join("remote.db")).unwrap();
    let pending = engine.lock().unwrap().pending().unwrap();
    let device_id = pending[0].device_id.clone();
    let payload = serde_json::to_string(&pending).unwrap();
    assert!(!payload.contains("answer") && !payload.contains("hello"));
    sync::push(
        &server,
        PushRequest {
            protocol_version: 4,
            device_id,
            device_name: "fixture".into(),
            records: pending,
        },
    )
    .unwrap();
    let changes = sync::changes(&server, 0, 500).unwrap();
    sync::apply_changes(&remote, &changes).unwrap();
    let synced = remote.records("all").unwrap();
    assert_eq!(synced.len(), 2);
    assert!(synced.iter().all(|r| r.harness == "cherry"));
    let repeat = engine.scan(false).unwrap();
    assert_eq!(repeat.imported, 0);
    assert_eq!(repeat.updated, 0);
    assert_eq!(repeat.deferred_files, 0);
    db.execute(
        "UPDATE ai_usage_record SET no_cache_tokens=NULL WHERE id='one'",
        [],
    )
    .unwrap();
    assert_eq!(engine.scan(false).unwrap().deferred_files, 1);
    assert_eq!(engine.lock().unwrap().records("local").unwrap().len(), 2);
}

#[test]
fn deepseek_pricing_backfills_old_unpriced_facts_and_keeps_reported_costs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cherrystudio.sqlite");
    let db = seed(&path);
    db.execute_batch("INSERT INTO ai_usage_record VALUES('deep','deep-request','invocation',1,'chat','msg','deepseek-v4-flash','deepseek',100,30,130,75,25,NULL,10,0.5,'USD','computed',1793000000000,'language');").unwrap();
    let engine = Engine::open(&dir.path().join("scope.db")).unwrap();
    {
        let store = engine.lock().unwrap();
        let mut config = store.config().unwrap();
        config.sources = vec![Source {
            id: "cherry-fixture".into(),
            harness: "cherry".into(),
            root: path.to_string_lossy().into(),
            enabled: true,
        }];
        store.set_meta("config", &config).unwrap();
    }
    assert_eq!(engine.scan(false).unwrap().imported, 3);
    let store = engine.lock().unwrap();
    let mut old = store.record("cherry:deep").unwrap().unwrap();
    assert!(old.cost.amount_usd.is_none()); // no CNY exchange rate configured yet
    old.parser_version = "cherry-sqlite-v2".into();
    store.upsert(old, path.to_str().unwrap()).unwrap();
    let previous = store.record("cherry:deep").unwrap().unwrap();
    let reported = store.record("cherry:one").unwrap().unwrap();
    store
        .conn
        .execute(
            "UPDATE file_state SET parser_version='cherry-sqlite-v2' WHERE path=?1",
            [path.to_str().unwrap()],
        )
        .unwrap();
    let mut settings = pricing::deepseek::default_settings();
    settings
        .exchange_rates
        .insert("CNY".into(), "6.68249010".into());
    store.set_meta("pricing_settings", &settings).unwrap();
    drop(store);

    let report = engine.scan(false).unwrap();
    assert!(report.errors.is_empty());
    let current = engine
        .lock()
        .unwrap()
        .record("cherry:deep")
        .unwrap()
        .unwrap();
    assert_eq!(current.id, previous.id);
    assert_eq!(current.tokens, previous.tokens);
    assert_eq!(current.provider.as_deref(), Some("deepseek"));
    assert_eq!(current.cost.method, "estimated");
    assert_eq!(current.cost.price_source, "deepseek_official");
    assert_eq!(
        current.cost.pricing_model.as_deref(),
        Some("deepseek-flash")
    );
    assert!(current.cost.amount_usd.is_some());
    assert_eq!(
        current
            .cost
            .conversion
            .as_ref()
            .unwrap()
            .units_per_usd
            .as_deref(),
        Some("6.68249010")
    );
    assert_eq!(
        engine
            .lock()
            .unwrap()
            .record("cherry:one")
            .unwrap()
            .unwrap()
            .cost,
        reported.cost
    );
    assert_eq!(engine.scan(true).unwrap().updated, 0);
    assert_eq!(
        engine
            .lock()
            .unwrap()
            .record("cherry:deep")
            .unwrap()
            .unwrap(),
        current
    );
}

#[test]
#[ignore = "read-only inspection of a separately supplied local database"]
fn inspect_local_cherry() {
    let path = std::env::var_os("CHERRY_READONLY_DB").expect("CHERRY_READONLY_DB");
    let path = Path::new(&path);
    let sessions = Cherry.read_metadata(path).unwrap();
    let (events, _, errors, complete) = Cherry.read_usage_source(path).unwrap();
    assert!(complete && errors.is_empty());
    let parsed = Cherry.parse(path, &events, &TimelineIndex::new()).unwrap();
    println!(
        "sessions={}, usage_rows={}, eligible={}, diagnostics={:?}",
        sessions.len(),
        events.len(),
        parsed.usage.len(),
        parsed.diagnostics
    );
}

#[test]
#[ignore = "requires a task-created isolated database copy and a local Cherry database"]
fn inspect_isolated_pricing_backfill() {
    let copy = PathBuf::from(
        std::env::var_os("CHERRY_SCOPE_ISOLATED_DB").expect("CHERRY_SCOPE_ISOLATED_DB"),
    );
    let source = PathBuf::from(std::env::var_os("CHERRY_READONLY_DB").expect("CHERRY_READONLY_DB"));
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let allowed = repo.join("output/cherry-pricing").canonicalize().unwrap();
    let copy = copy.canonicalize().unwrap();
    assert!(copy.starts_with(allowed) && copy.file_name().unwrap() == "isolated-pricing.db");
    let engine = Engine::open(&copy).unwrap();
    let before: BTreeMap<_, _> = {
        let store = engine.lock().unwrap();
        let mut stmt = store.conn.prepare("SELECT id FROM usage_index WHERE origin='local' AND harness='cherry' AND deleted=0 ORDER BY id").unwrap();
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        ids.into_iter()
            .map(|id| (id.clone(), store.record(&id).unwrap().unwrap()))
            .collect()
    };
    assert!(!before.is_empty());
    {
        let store = engine.lock().unwrap();
        let mut config = store.config().unwrap();
        config.sources = vec![Source {
            id: "cherry-isolated".into(),
            harness: "cherry".into(),
            root: source.to_string_lossy().into(),
            enabled: true,
        }];
        config.server_url.clear();
        config.server_token.clear();
        store.set_meta("config", &config).unwrap();
    }
    let report = engine.scan_harness("cherry", false).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let store = engine.lock().unwrap();
    let mut priced = 0;
    for (id, old) in &before {
        let now = store.record(id).unwrap().unwrap();
        assert_eq!(
            (
                &now.id,
                &now.session_id,
                &now.native_event_id,
                &now.tokens,
                &now.model,
                &now.provider,
                now.occurred_at
            ),
            (
                &old.id,
                &old.session_id,
                &old.native_event_id,
                &old.tokens,
                &old.model,
                &old.provider,
                old.occurred_at
            )
        );
        if old.cost.amount_usd.is_some() {
            assert_eq!(now.cost, old.cost);
        }
        if old.cost.amount_usd.is_none()
            && now.cost.method == "estimated"
            && now.cost.amount_usd.is_some()
        {
            priced += 1;
        }
    }
    assert!(
        priced > 0,
        "No previously unpriced Cherry facts were priced"
    );
    assert_eq!(
        store
            .conn
            .query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    drop(store);
    let repeat = engine.scan_harness("cherry", true).unwrap();
    assert_eq!((repeat.imported, repeat.updated), (0, 0));
    println!(
        "Existing Cherry facts={}, newly priced={}, repeat updated={}",
        before.len(),
        priced,
        repeat.updated
    );
}
