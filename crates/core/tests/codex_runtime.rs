use harness_scope_core::{
    model::{RequestMetadata, Source},
    Engine,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{fs, path::Path};

const THREAD: &str = "01a164e9-e38b-72b3-bd42-2255a9bec935";
const TURN: &str = "01a164ea-7d4e-79d2-a028-c2458c5f12ff";
const NEXT: &str = "01a164ea-7d4e-79d2-a028-c2458c5f1300";
const MODEL: &str = "gpt-6-astra";

fn rollout() -> Vec<Value> {
    vec![
        json!({"type":"session_meta","payload":{"id":THREAD,"model_provider":"openai"}}),
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":TURN}}),
        json!({"type":"turn_context","payload":{"turn_id":TURN,"model":MODEL,"effort":"xhigh"}}),
        usage(1),
        usage(2),
    ]
}
fn usage(n: u64) -> Value {
    json!({"timestamp":format!("2026-10-23T00:00:{n:02}Z"),"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"output_tokens":10},"total_token_usage":{"input_tokens":n*100,"output_tokens":n*10}}}})
}
fn write(path: &Path, rows: &[Value]) {
    fs::write(
        path,
        rows.iter().map(|v| format!("{v}\n")).collect::<String>(),
    )
    .unwrap();
}
fn setup(home: &Path, rows: &[Value]) -> Engine {
    let root = home.join("sessions");
    fs::create_dir_all(&root).unwrap();
    write(&root.join("fixture.jsonl"), rows);
    let engine = Engine::open(&home.join("scope.db")).unwrap();
    let store = engine.lock().unwrap();
    let mut config = store.config().unwrap();
    config.sources = vec![Source {
        id: "fixture".into(),
        harness: "codex".into(),
        root: root.to_string_lossy().into(),
        enabled: true,
    }];
    store.set_meta("config", &config).unwrap();
    drop(store);
    engine
}
fn logs(home: &Path) -> Connection {
    let conn = Connection::open(home.join("logs_2.sqlite")).unwrap();
    conn.execute_batch("CREATE TABLE logs(id INTEGER PRIMARY KEY,ts INTEGER,ts_nanos INTEGER,thread_id TEXT,target TEXT,feedback_log_body TEXT); CREATE INDEX thread_idx ON logs(thread_id);").unwrap();
    conn
}
fn tag(conn: &Connection, thread: &str, turn: &str, model: &str, tier: Value, second: i64) {
    let at = chrono::DateTime::parse_from_rfc3339("2026-10-23T00:00:00Z")
        .unwrap()
        .timestamp()
        + second;
    let body = format!("session_loop{{thread_id={thread}}}:submission{{conversation.id={thread} turn.id={turn}}}:model_step{{turn_id={turn} model={model}}}: feedback_tags: {}", json!({"service_tier":tier,"private":"secret-not-retained"}));
    conn.execute("INSERT INTO logs(ts,ts_nanos,thread_id,target,feedback_log_body) VALUES(?1,0,?2,'feedback_tags',?3)", params![at,thread,body]).unwrap();
}
fn speeds(engine: &Engine) -> Vec<Option<String>> {
    let mut records = engine.lock().unwrap().local_records().unwrap();
    records.sort_by_key(|r| r.occurred_at);
    records
        .iter()
        .map(|r| {
            r.request_metadata
                .as_ref()
                .and_then(RequestMetadata::speed)
                .map(str::to_owned)
        })
        .collect()
}
#[test]
fn first_turn_fast_and_standard_are_recovered_from_matching_runtime_settings() {
    for (tier, speed) in [("priority", "fast"), ("default", "standard")] {
        let dir = tempfile::tempdir().unwrap();
        let engine = setup(dir.path(), &rollout());
        let conn = logs(dir.path());
        tag(&conn, THREAD, TURN, MODEL, json!(tier), 0);
        let report = engine.scan(false).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(report.imported, 2);
        assert_eq!(speeds(&engine), vec![Some(speed.into()); 2]);
        let store = engine.lock().unwrap();
        for record in store.local_records().unwrap() {
            assert_eq!(record.tokens.total(), 110);
            assert_eq!(
                record.request_metadata.unwrap().reasoning_effort.as_deref(),
                Some("xhigh")
            );
        }
        let metadata: Vec<String> = store
            .conn
            .prepare("SELECT value FROM meta WHERE key LIKE 'codex_runtime_%'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(metadata.len(), 1);
        assert!(!metadata[0].contains("secret-not-retained"));
    }
}
#[test]
fn late_runtime_evidence_backfills_once_and_survives_log_pruning_and_restart() {
    let dir = tempfile::tempdir().unwrap();
    let engine = setup(dir.path(), &rollout());
    engine.scan(false).unwrap();
    let before = engine.lock().unwrap().local_records().unwrap();
    assert_eq!(speeds(&engine), vec![None; 2]);
    let conn = logs(dir.path());
    tag(&conn, THREAD, TURN, MODEL, json!("priority"), 0);
    let report = engine.scan(false).unwrap();
    assert_eq!((report.updated, report.imported), (2, 0));
    let after = engine.lock().unwrap().local_records().unwrap();
    for old in &before {
        let new = after.iter().find(|r| r.id == old.id).unwrap();
        assert_eq!(new.tokens, old.tokens);
        assert_eq!(new.cost, old.cost);
        assert_eq!(new.revision, old.revision + 1);
        assert_eq!(new.model, old.model);
        assert_eq!(new.occurred_at, old.occurred_at);
    }
    assert_eq!(engine.scan(false).unwrap().updated, 0);
    tag(&conn, THREAD, TURN, MODEL, json!("priority"), 1);
    assert_eq!(engine.scan(false).unwrap().updated, 0);
    conn.execute("DELETE FROM logs", []).unwrap();
    drop(conn);
    fs::remove_file(dir.path().join("logs_2.sqlite")).unwrap();
    drop(engine);
    let engine = Engine::open(&dir.path().join("scope.db")).unwrap();
    assert_eq!(engine.scan(true).unwrap().updated, 0);
    assert_eq!(engine.lock().unwrap().local_records().unwrap(), after);
}
#[test]
fn runtime_evidence_is_bound_to_source_thread_turn_model_and_time() {
    for wrong in ["thread", "turn", "model", "time", "incompatible", "source"] {
        let dir = tempfile::tempdir().unwrap();
        let engine = setup(dir.path(), &rollout());
        if wrong == "incompatible" {
            fs::write(dir.path().join("logs_2.sqlite"), "not sqlite").unwrap();
        } else {
            let home = if wrong == "source" {
                dir.path().join("other")
            } else {
                dir.path().to_owned()
            };
            fs::create_dir_all(&home).unwrap();
            let conn = logs(&home);
            tag(
                &conn,
                if wrong == "thread" { NEXT } else { THREAD },
                if wrong == "turn" { NEXT } else { TURN },
                if wrong == "model" {
                    "different-model"
                } else {
                    MODEL
                },
                json!("priority"),
                if wrong == "time" { 3 } else { 0 },
            );
        }
        let report = engine.scan(false).unwrap();
        assert!(report.errors.is_empty(), "{wrong}: {:?}", report.errors);
        assert_eq!(report.imported, 2);
        assert_eq!(speeds(&engine), vec![None; 2], "{wrong}");
    }
}
#[test]
fn conflicting_runtime_tiers_do_not_override_explicit_context_or_guess() {
    let dir = tempfile::tempdir().unwrap();
    let mut rows = rollout();
    rows.extend([
        json!({"type":"event_msg","payload":{"type":"task_started","turn_id":NEXT}}),
        json!({"type":"turn_context","payload":{"model":MODEL,"service_tier":"default"}}),
        usage(3),
    ]);
    let engine = setup(dir.path(), &rows);
    let conn = logs(dir.path());
    tag(&conn, THREAD, TURN, MODEL, json!("priority"), 0);
    tag(&conn, THREAD, NEXT, MODEL, json!("priority"), 0);
    engine.scan(false).unwrap();
    assert_eq!(
        speeds(&engine),
        vec![
            Some("fast".into()),
            Some("fast".into()),
            Some("standard".into())
        ]
    );
    tag(&conn, THREAD, TURN, MODEL, json!("default"), 1);
    assert_eq!(engine.scan(false).unwrap().updated, 2);
    assert_eq!(speeds(&engine), vec![None, None, Some("standard".into())]);
}
#[test]
fn compressed_rollout_preserves_already_collected_usage_and_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let mut rows = rollout();
    rows.truncate(3);
    rows.push(json!({"type":"token_usage_record","timestamp":"2026-10-23T00:00:01Z","payload":{"turn_id":TURN,"response_id":"fixture","usage":{"input_tokens":100,"output_tokens":10}}}));
    rows.push(json!({"type":"event_msg","timestamp":"2026-10-23T00:00:02Z","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":0,"output_tokens":0}}}}));
    let engine = setup(dir.path(), &rows);
    let path = dir.path().join("sessions/fixture.jsonl");
    let conn = logs(dir.path());
    tag(&conn, THREAD, TURN, MODEL, json!("priority"), 0);
    assert_eq!(engine.scan(false).unwrap().imported, 1);
    let data = fs::read(&path).unwrap();
    fs::write(
        path.with_extension("jsonl.zst"),
        zstd::stream::encode_all(data.as_slice(), 1).unwrap(),
    )
    .unwrap();
    fs::remove_file(path).unwrap();
    let report = engine.scan(true).unwrap();
    assert_eq!(report.imported, 0);
    assert_eq!(report.updated, 0);
    assert_eq!(speeds(&engine), vec![Some("fast".into())]);
}
