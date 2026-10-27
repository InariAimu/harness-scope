use harness_scope_core::{
    harness::{opencode::OpenCode, HarnessAdapter, TimelineIndex},
    model::Source,
    Engine,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::path::Path;

const V1: &str = include_str!("../../../tests/fixtures/opencode/v1.json");
const V2: &str = include_str!("../../../tests/fixtures/opencode/v2.json");

fn seed(path: &Path, fixture: &str) -> Connection {
    let c = Connection::open(path).unwrap();
    c.execute_batch("PRAGMA journal_mode=WAL;
        CREATE TABLE IF NOT EXISTS session(id TEXT PRIMARY KEY,title TEXT,directory TEXT,time_created INTEGER,time_updated INTEGER);
        CREATE TABLE IF NOT EXISTS message(id TEXT PRIMARY KEY,session_id TEXT,time_created INTEGER,data TEXT);
        CREATE TABLE IF NOT EXISTS part(id TEXT PRIMARY KEY,message_id TEXT,session_id TEXT,time_created INTEGER,data TEXT);").unwrap();
    let f: Value = serde_json::from_str(fixture).unwrap();
    let v2 = f["messages"][0]["type"].is_string();
    if v2 {
        c.execute_batch("CREATE TABLE IF NOT EXISTS session_message(id TEXT PRIMARY KEY,session_id TEXT,type TEXT,data TEXT,seq INTEGER);
            CREATE TABLE IF NOT EXISTS session_v2(id TEXT PRIMARY KEY,title TEXT,directory TEXT,time_created INTEGER,time_updated INTEGER);").unwrap();
    }
    for s in f["sessions"].as_array().unwrap() {
        let table = if v2 { "session_v2" } else { "session" };
        c.execute(
            &format!("INSERT OR REPLACE INTO {table} VALUES(?1,?2,?3,?4,?5)"),
            params![
                s["id"].as_str(),
                s["title"].as_str(),
                s["directory"].as_str(),
                s["created"].as_i64(),
                s["updated"].as_i64()
            ],
        )
        .unwrap();
    }
    for (seq, m) in f["messages"].as_array().unwrap().iter().enumerate() {
        if v2 {
            c.execute(
                "INSERT OR REPLACE INTO session_message VALUES(?1,?2,?3,?4,?5)",
                params![
                    m["id"].as_str(),
                    m["sessionId"].as_str(),
                    m["type"].as_str(),
                    m["data"].to_string(),
                    seq as i64
                ],
            )
            .unwrap();
        } else {
            c.execute(
                "INSERT OR REPLACE INTO message VALUES(?1,?2,?3,?4)",
                params![
                    m["id"].as_str(),
                    m["sessionId"].as_str(),
                    m["data"]["time"]["created"].as_i64(),
                    m["data"].to_string()
                ],
            )
            .unwrap();
        }
    }
    if let Some(parts) = f["parts"].as_array() {
        for (seq, p) in parts.iter().enumerate() {
            c.execute(
                "INSERT INTO part VALUES(?1,?2,?3,?4,?5)",
                params![
                    p["id"].as_str(),
                    p["messageId"].as_str(),
                    p["sessionId"].as_str(),
                    seq as i64,
                    p["data"].to_string()
                ],
            )
            .unwrap();
        }
    }
    c
}

fn engine(dir: &Path, source: &Path) -> Engine {
    let e = Engine::open(&dir.join("scope.db")).unwrap();
    let s = e.lock().unwrap();
    let mut c = s.config().unwrap();
    c.sources = vec![Source {
        id: "opencode-fixture".into(),
        harness: "opencode".into(),
        root: source.to_string_lossy().into(),
        enabled: true,
    }];
    s.set_meta("config", &c).unwrap();
    drop(s);
    e
}

#[test]
fn v1_wal_usage_and_tools_are_read_without_mutating_source() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("opencode.db");
    let c = seed(&path, V1); // keep WAL uncheckpointed and the writer open
    let before = c.total_changes();
    let (events, _, errors, complete) = OpenCode.read_source(&path).unwrap();
    assert!(complete && errors.is_empty());
    let parsed = OpenCode
        .parse(&path, &events, &TimelineIndex::new())
        .unwrap();
    assert!(parsed.complete);
    assert_eq!(parsed.usage.len(), 1); // step-finish duplicates the message's counters
    let u = &parsed.usage[0];
    assert_eq!(u.provider.as_deref(), Some("deepseek"));
    assert_eq!(
        (
            u.tokens.input,
            u.tokens.output,
            u.tokens.reasoning,
            u.tokens.total()
        ),
        (100, 25, 5, 195)
    );
    assert_eq!(u.reported_cost.as_deref(), Some("0.00012345"));
    let messages = OpenCode.messages_for_session(&events, Some("ses_fixture"));
    assert_eq!(messages[0]["content"], "Synthetic prompt");
    assert_eq!(messages[1]["blocks"][1]["rawName"], "bash");
    assert_eq!(messages[1]["blocks"][2]["preview"], "synthetic");
    assert_eq!(messages[1]["meta"]["outputTokens"], 25);
    assert_eq!(c.total_changes(), before);
}

#[test]
fn a_pure_v2_database_lists_sessions_without_legacy_tables() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("opencode.db");
    let c = seed(&path, V2);
    c.execute_batch("DROP TABLE session; DROP TABLE message; DROP TABLE part;")
        .unwrap();
    let e = engine(tmp.path(), &path);
    assert_eq!(e.scan(false).unwrap().imported, 2);
    let sessions = e.sessions().unwrap();
    assert_eq!(sessions.len(), 2);
    assert!(sessions
        .iter()
        .all(|s| s.created_at.unwrap() > 1_000_000_000_000));
    assert!(sessions
        .iter()
        .any(|s| s.resume_command.as_deref() == Some("opencode --session ses_second")));
}

#[test]
fn usage_projection_matches_full_reader_and_tolerates_unrelated_invalid_text() {
    for fixture in [V1, V2] {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("opencode.db");
        let _c = seed(&path, fixture);
        let full = OpenCode.read_source(&path).unwrap();
        let projected = OpenCode.read_usage_source(&path).unwrap();
        let a = OpenCode
            .parse(&path, &full.0, &TimelineIndex::new())
            .unwrap();
        let b = OpenCode
            .parse(&path, &projected.0, &TimelineIndex::new())
            .unwrap();
        assert_eq!(format!("{:?}", a.usage), format!("{:?}", b.usage));
    }
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("opencode.db");
    let c = seed(&path, V2);
    let raw: String = c
        .query_row(
            "SELECT data FROM session_message WHERE id='msg_answer'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let bad_text = raw.replace("Synthetic reasoning", r"\ud800");
    c.execute(
        "UPDATE session_message SET data=?1 WHERE id='msg_answer'",
        [&bad_text],
    )
    .unwrap();
    assert!(!OpenCode.read_source(&path).unwrap().3);
    let e = engine(tmp.path(), &path);
    let report = e.scan(false).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.imported, 2);
    assert_eq!(e.scan(false).unwrap().imported, 0);
    // The same malformed surrogate in a required token field is not accepted.
    let bad_counter = raw.replace("\"input\":100", r#""input":"\ud800""#);
    assert_ne!(bad_counter, raw);
    c.execute(
        "UPDATE session_message SET data=?1 WHERE id='msg_answer'",
        [&bad_counter],
    )
    .unwrap();
    let report = e.scan(false).unwrap();
    assert!(report.deferred_files > 0 && !report.errors.is_empty());
    assert_eq!(e.lock().unwrap().records("local").unwrap().len(), 2);
}

#[test]
fn projection_upgrade_refreshes_old_read_cache_without_revising_facts() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("opencode.db");
    let _c = seed(&path, V2);
    let e = engine(tmp.path(), &path);
    assert_eq!(e.scan(false).unwrap().imported, 2);
    let before = e.lock().unwrap().records("local").unwrap();
    {
        let s = e.lock().unwrap();
        let payload: String = s
            .conn
            .query_row(
                "SELECT payload FROM usage_read_cache WHERE path=?1",
                [path.to_string_lossy().as_ref()],
                |r| r.get(0),
            )
            .unwrap();
        let mut cache: Value = serde_json::from_str(&payload).unwrap();
        cache["version"] = json!("usage-read-v2:opencode-sqlite-v3-provider");
        s.conn
            .execute(
                "UPDATE usage_read_cache SET payload=?1 WHERE path=?2",
                params![cache.to_string(), path.to_string_lossy().as_ref()],
            )
            .unwrap();
    }
    let report = e.scan(false).unwrap();
    assert_eq!((report.imported, report.updated), (0, 0));
    assert_eq!(e.lock().unwrap().records("local").unwrap(), before);
    let s = e.lock().unwrap();
    let payload: String = s
        .conn
        .query_row(
            "SELECT payload FROM usage_read_cache WHERE path=?1",
            [path.to_string_lossy().as_ref()],
            |r| r.get(0),
        )
        .unwrap();
    assert!(serde_json::from_str::<Value>(&payload).unwrap()["version"]
        .as_str()
        .unwrap()
        .starts_with("usage-read-v3-opencode-projection:"));
}

#[test]
fn v1_to_v2_keeps_ids_and_deduplicates_migrated_sessions() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("opencode.db");
    let _writer = seed(&path, V1);
    let e = engine(tmp.path(), &path);
    assert_eq!(e.scan(false).unwrap().imported, 1);
    let first = e.lock().unwrap().local_records().unwrap().remove(0);
    let _v2 = seed(&path, V2);
    let report = e.scan(false).unwrap();
    assert_eq!((report.imported, report.updated), (1, 0));
    let records = e.lock().unwrap().records("local").unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records.iter().map(|r| r.tokens.total()).sum::<u64>(), 230);
    assert_eq!(
        records.iter().find(|r| r.id == first.id).unwrap().revision,
        1
    );
    assert!(records.iter().all(|r| r.cost.method == "reported"));
    assert_eq!(
        records
            .iter()
            .find(|r| r.id == first.id)
            .unwrap()
            .provider
            .as_deref(),
        Some("deepseek")
    );
    assert!(records
        .iter()
        .any(|r| r.cost.amount_usd.as_deref() == Some("0")));
    let sessions = e.sessions().unwrap();
    assert_eq!(sessions.len(), 2);
    assert!(sessions.iter().all(|s| !s.can_delete));
    let args = json!({"providerId":"opencode","sourcePath":path,"sessionId":"ses_fixture"});
    let transcript = e.execute("session_messages", args.clone()).unwrap();
    assert_eq!(transcript[1]["blocks"][1]["rawName"], "shell");
    assert_eq!(transcript[1]["blocks"][2]["preview"], "synthetic");
    assert_eq!(
        e.execute(
            "session_messages",
            json!({"providerId":"opencode","sourcePath":path,"sessionId":"ses_second"})
        )
        .unwrap()
        .as_array()
        .unwrap()
        .len(),
        1
    );
    assert!(e.execute("delete_session", args).is_err());
    assert!(path.exists());
    assert_eq!(e.scan(true).unwrap().updated, 0);
    assert_eq!(e.lock().unwrap().pending_count().unwrap(), 2);
    let wire = serde_json::to_string(&e.lock().unwrap().pending().unwrap()).unwrap();
    assert!(!wire.contains("Synthetic answer") && !wire.contains("/synthetic/project"));
}

#[test]
fn incomplete_and_invalid_rows_preserve_usage_and_completed_corrections_update_reported_cost() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("opencode.db");
    let c = seed(&path, V2);
    let e = engine(tmp.path(), &path);
    assert_eq!(e.scan(false).unwrap().imported, 2);
    c.execute(
        "UPDATE session_message SET data='[]' WHERE id='msg_answer'",
        [],
    )
    .unwrap();
    let report = e.scan(false).unwrap();
    assert!(report.deferred_files > 0 && !report.errors.is_empty());
    assert_eq!(e.lock().unwrap().records("local").unwrap().len(), 2);
    let f: Value = serde_json::from_str(V2).unwrap();
    let mut d = f["messages"][1]["data"].clone();
    d["time"]["completed"] = Value::Null;
    c.execute(
        "UPDATE session_message SET data=?1 WHERE id='msg_answer'",
        [d.to_string()],
    )
    .unwrap();
    assert_eq!(e.scan(false).unwrap().deferred_files, 1);
    d["time"]["completed"] = json!(1792112405000i64);
    d["tokens"]["input"] = json!(200);
    d["cost"] = json!(0.0002469);
    c.execute(
        "UPDATE session_message SET data=?1 WHERE id='msg_answer'",
        [d.to_string()],
    )
    .unwrap();
    assert_eq!(e.scan(false).unwrap().updated, 1);
    let records = e.lock().unwrap().records("local").unwrap();
    let r = records
        .iter()
        .find(|r| r.native_event_id.as_deref() == Some("msg_answer"))
        .unwrap();
    assert_eq!(r.revision, 2);
    assert_eq!(r.cost.method, "reported");
    assert_eq!(r.cost.amount_usd.as_deref(), Some("0.0002469"));
    d["cost"] = json!(5e-8);
    c.execute(
        "UPDATE session_message SET data=?1 WHERE id='msg_answer'",
        [d.to_string()],
    )
    .unwrap();
    assert_eq!(e.scan(false).unwrap().updated, 1);
    let r = e.lock().unwrap().record(&r.id).unwrap().unwrap();
    assert_eq!(r.cost.amount_usd.as_deref(), Some("0.00000005"));
    d["tokens"]["input"] = json!(-1);
    c.execute(
        "UPDATE session_message SET data=?1 WHERE id='msg_answer'",
        [d.to_string()],
    )
    .unwrap();
    let first = e.scan(false).unwrap();
    assert_eq!(first.deferred_files, 1);
    let repeat = e.scan(false).unwrap();
    assert_eq!(repeat.deferred_files, 1);
    assert_eq!(repeat.errors, first.errors);
    assert_eq!(repeat.imported, 0);
    let s = e.lock().unwrap();
    let cached: String = s
        .conn
        .query_row(
            "SELECT payload FROM usage_read_cache WHERE path=?1",
            [path.to_string_lossy().as_ref()],
            |r| r.get(0),
        )
        .unwrap();
    assert!(serde_json::from_str::<Value>(&cached).unwrap()["deferred"].is_object());
    drop(s);
    assert_eq!(e.scan(true).unwrap().deferred_files, 1);
    assert_eq!(
        e.lock().unwrap().record(&r.id).unwrap().unwrap().revision,
        3
    );
    d["tokens"]["input"] = json!(201);
    c.execute(
        "UPDATE session_message SET data=?1 WHERE id='msg_answer'",
        [d.to_string()],
    )
    .unwrap();
    let repaired = e.scan(false).unwrap();
    assert_eq!(repaired.updated, 1);
    assert_eq!(repaired.deferred_files, 0);
    assert!(repaired.errors.is_empty());
}

#[test]
fn scoped_reader_ignores_other_corrupt_sessions_and_wal_changes_invalidate_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("opencode.db");
    let c = seed(&path, V2);
    c.execute(
        "INSERT INTO session_message VALUES('bad','unrelated','assistant','broken JSON',99)",
        [],
    )
    .unwrap();
    let e = engine(tmp.path(), &path);
    assert!(!e.sessions().unwrap().is_empty());
    let args = json!({"providerId":"opencode","sourcePath":path,"sessionId":"ses_fixture"});
    let first = e.execute("session_transcript", args.clone()).unwrap();
    assert_eq!(first["header"]["cached"], false);
    assert_eq!(
        e.execute("session_transcript", args.clone()).unwrap()["header"]["cached"],
        true
    );
    let old_main = std::fs::metadata(&path).unwrap().len();
    c.execute(
        "UPDATE session_v2 SET title='WAL-only updated title' WHERE id='ses_fixture'",
        [],
    )
    .unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), old_main);
    assert!(e
        .sessions()
        .unwrap()
        .iter()
        .any(|s| s.title.as_deref() == Some("WAL-only updated title")));
    assert_eq!(
        e.execute("session_transcript", args).unwrap()["header"]["cached"],
        false
    );
    assert!(!OpenCode.read_source(&path).unwrap().3); // Full usage ingestion still reports corruption.
}
