use harness_scope_core::{
    harness::{claude::Claude, codex::Codex, files, HarnessAdapter, TimelineIndex},
    migration::{self, MigrationInput},
    model::*,
    pricing,
    store::Store,
    sync, Engine,
};
use serde_json::{json, Value};
use std::{fs, path::Path};
const CLAUDE: &str = include_str!("../../../tests/fixtures/claude/session.jsonl");
const CODEX: &str = include_str!("../../../tests/fixtures/codex/session.jsonl");
fn values(s: &str) -> Vec<Value> {
    s.lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect()
}
fn record(id: &str) -> UsageRecord {
    UsageRecord {
        timing: None,
        request_metadata: None,
        provider: None,
        device_id: "test-device".into(),
        id: id.into(),
        harness: "claude".into(),
        parser_version: "v1".into(),
        session_id: Some("session".into()),
        native_event_id: Some(id.into()),
        occurred_at: 1792112400,
        model: "claude-sonnet-4-5".into(),
        grain: "event".into(),
        count: 1,
        tokens: Tokens {
            input: 10,
            output: 20,
            ..Default::default()
        },
        cost: Cost::default(),
        record_source: "session".into(),
        revision: 1,
        deleted: false,
        status_code: None,
        legacy_date: None,
        legacy_timezone: None,
    }
}
fn configured_engine(dir: &Path, root: &Path, harness: &str) -> Engine {
    let e = Engine::open(&dir.join("scope.db")).unwrap();
    let s = e.lock().unwrap();
    let mut c = s.config().unwrap();
    c.sources = vec![Source {
        id: "fixture".into(),
        harness: harness.into(),
        root: root.to_string_lossy().into(),
        enabled: true,
    }];
    c.timezone = "UTC".into();
    s.set_meta("config", &c).unwrap();
    drop(s);
    e
}

#[test]
fn claude_blocks_are_one_revisable_event() {
    let r = Claude
        .parse(
            Path::new("fixture.jsonl"),
            &values(CLAUDE),
            &TimelineIndex::new(),
        )
        .unwrap();
    assert!(r.complete);
    assert_eq!(r.usage.len(), 1);
    assert_eq!(r.usage[0].tokens.output, 30);
    assert_eq!(r.usage[0].tokens.total(), 220);
    assert_eq!(r.usage[0].tokens.cache_write1h, 20);
}
#[test]
fn codex_cumulative_cache_and_reasoning_are_not_double_counted() {
    let r = Codex
        .parse(
            Path::new("fixture.jsonl"),
            &values(CODEX),
            &TimelineIndex::new(),
        )
        .unwrap();
    assert!(r.complete);
    assert_eq!(r.usage.len(), 2);
    assert_eq!(r.usage.iter().map(|r| r.tokens.total()).sum::<u64>(), 190);
    assert_eq!(r.usage[1].tokens.input, 30);
    assert_eq!(r.usage[1].tokens.reasoning, 5);
}
#[test]
fn missing_codex_parent_is_deferred() {
    let mut lines = values(CODEX);
    lines[0]["payload"]["forked_from_id"] = json!("missing-parent");
    let r = Codex
        .parse(Path::new("f"), &lines, &TimelineIndex::new())
        .unwrap();
    assert!(!r.complete);
    assert!(r.usage.is_empty());
    assert!(!r.diagnostics.is_empty());
}
#[test]
fn modern_codex_cache_write_alias_preserves_identity_and_paired_usage_is_not_duplicated() {
    let fixture = include_str!("../../../tests/fixtures/codex/modern.jsonl");
    let parsed = Codex
        .parse(
            Path::new("modern.jsonl"),
            &values(fixture),
            &TimelineIndex::new(),
        )
        .unwrap();
    assert!(parsed.complete);
    assert_eq!(parsed.usage.len(), 1);
    let usage = &parsed.usage[0];
    assert_eq!(usage.id, "codex:v6:codex-modern:token:1");
    assert_eq!(
        (
            usage.tokens.input,
            usage.tokens.cache_read,
            usage.tokens.cache_write,
            usage.tokens.output
        ),
        (80, 30, 10, 20)
    );
    assert_eq!(usage.tokens.total(), 140);
    assert_eq!(usage.tokens.reasoning, 5);
}

#[test]
fn session_queries_do_not_fabricate_http_timing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("logs");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("session.jsonl"), CODEX).unwrap();
    let e = configured_engine(tmp.path(), &root, "codex");
    e.scan(false).unwrap();
    let logs = e.execute("usage_query", json!({"kind":"logs"})).unwrap();
    assert_eq!(logs["total"], 2);
    for row in logs["data"].as_array().unwrap() {
        assert_eq!(row["timingMethod"], "estimated");
        assert!(row["latencyMs"].as_i64().is_some_and(|ms| ms > 0));
        assert!(row["firstTokenMs"].is_null());
        assert_eq!(row["durationMs"], row["latencyMs"]);
        assert_eq!(row["isStreaming"], false);
    }
}
#[test]
fn fork_history_is_skipped_and_new_usage_is_counted() {
    let parent = values(CODEX);
    let mut child = parent.clone();
    child[0]["payload"]["id"] = json!("child");
    child[0]["payload"]["forked_from_id"] = json!("codex-fixture");
    child[0]["timestamp"] = json!("2026-10-16T02:00:05Z");
    child.push(json!({"type":"event_msg","timestamp":"2026-10-16T02:01:01Z","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":170,"cached_input_tokens":30,"output_tokens":45,"reasoning_output_tokens":8},"last_token_usage":{"input_tokens":20,"cached_input_tokens":0,"output_tokens":5,"reasoning_output_tokens":0}}}}));
    let index = TimelineIndex::from([("codex:codex-fixture".into(), parent)]);
    let r = Codex.parse(Path::new("f"), &child, &index).unwrap();
    assert!(r.complete);
    assert_eq!(r.usage.len(), 1);
    assert_eq!(r.usage[0].tokens.total(), 25);
}
#[test]
fn cache_1h_cost_and_price_provenance_are_preserved() {
    let mut p = pricing::find(&pricing::builtin(), "claude-sonnet-4-5")
        .unwrap()
        .clone();
    p.input_cost_per_million = "3".into();
    p.output_cost_per_million = "15".into();
    p.cache_read_cost_per_million = "0.3".into();
    p.cache_creation_cost_per_million = "3.75".into();
    let tokens = Tokens {
        input: 100,
        output: 30,
        cache_read: 40,
        cache_write: 50,
        cache_write1h: 20,
        reasoning: 0,
    };
    let c = pricing::estimate(&tokens, &p).unwrap();
    assert_eq!(c.amount_usd.as_deref(), Some("0.0009945"));
    assert_eq!(c.price_source, "builtin");
    assert_eq!(c.snapshot, Some(p));
}
#[test]
fn namespace_date_and_effort_matching_follows_upstream() {
    let prices = pricing::builtin();
    assert_eq!(
        pricing::find(&prices, "openrouter/anthropic/claude-sonnet-4.5:free")
            .unwrap()
            .model_id,
        "claude-sonnet-4-5-20250929"
    );
    assert_eq!(
        pricing::find(&prices, "gpt-5.4-high").unwrap().model_id,
        "gpt-5.4"
    );
    assert!(pricing::find(&prices, "unknown").is_none());
}
#[test]
fn scanning_and_process_restart_are_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("logs");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("s.jsonl"), CLAUDE).unwrap();
    let e = configured_engine(temp.path(), &root, "claude");
    assert_eq!(e.scan(false).unwrap().imported, 1);
    assert_eq!(e.scan(true).unwrap().updated, 0);
    let device = e.lock().unwrap().device_id().unwrap();
    drop(e);
    let e = Engine::open(&temp.path().join("scope.db")).unwrap();
    assert_eq!(e.lock().unwrap().device_id().unwrap(), device);
    assert_eq!(e.scan(false).unwrap().imported, 0);
    assert_eq!(e.lock().unwrap().pending_count().unwrap(), 1);
}
#[test]
fn partial_tail_is_retried_and_original_cost_snapshot_survives_updates() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("logs");
    fs::create_dir(&root).unwrap();
    let p = root.join("s.jsonl");
    fs::write(&p, format!("{CLAUDE}{{\"type\":")).unwrap();
    let e = configured_engine(temp.path(), &root, "claude");
    let report = e.scan(false).unwrap();
    assert_eq!(report.imported, 1);
    assert_eq!(report.deferred_files, 1);
    let old = e.lock().unwrap().local_records().unwrap().pop().unwrap();
    let mut prices = e.lock().unwrap().prices().unwrap();
    for price in &mut prices {
        price.output_cost_per_million = "999".into()
    }
    e.lock().unwrap().set_meta("prices", &prices).unwrap();
    fs::write(
        &p,
        CLAUDE.replace("\"output_tokens\":30", "\"output_tokens\":40"),
    )
    .unwrap();
    assert_eq!(e.scan(false).unwrap().updated, 1);
    let new = e.lock().unwrap().local_records().unwrap().pop().unwrap();
    assert_eq!(new.revision, 2);
    assert_eq!(new.cost.snapshot, old.cost.snapshot);
    assert_eq!(new.tokens.output, 40);
}
#[test]
fn failed_transaction_and_old_ack_do_not_lose_new_usage() {
    let tmp = tempfile::tempdir().unwrap();
    let s = Store::open(&tmp.path().join("s.db")).unwrap();
    let mut r = record("one");
    r.device_id = s.device_id().unwrap();
    assert!(s
        .transaction(|| -> anyhow::Result<()> {
            s.upsert(r.clone(), "fixture")?;
            anyhow::bail!("abort")
        })
        .is_err());
    assert_eq!(s.pending_count().unwrap(), 0);
    s.transaction(|| s.upsert(r.clone(), "fixture")).unwrap();
    r.tokens.output = 30;
    s.transaction(|| s.upsert(r, "fixture")).unwrap();
    s.ack(&[Ack {
        id: "one".into(),
        revision: 1,
    }])
    .unwrap();
    assert_eq!(s.pending().unwrap()[0].revision, 2);
}
#[test]
fn upload_retries_stale_revisions_and_conflicts_are_atomic() {
    let tmp = tempfile::tempdir().unwrap();
    let s = Store::open(&tmp.path().join("s.db")).unwrap();
    let r = record("one");
    let request = |records| PushRequest {
        protocol_version: 1,
        device_id: "test-device".into(),
        device_name: "Test".into(),
        records,
    };
    sync::push(&s, request(vec![r.clone()])).unwrap();
    sync::push(&s, request(vec![r.clone()])).unwrap();
    assert_eq!(sync::changes(&s, 0, 500).unwrap().changes.len(), 1);
    let mut newer = r.clone();
    newer.revision = 2;
    newer.tokens.output = 30;
    sync::push(&s, request(vec![newer.clone()])).unwrap();
    sync::push(&s, request(vec![r.clone()])).unwrap();
    assert_eq!(sync::changes(&s, 0, 500).unwrap().changes.len(), 1);
    let mut bad = newer;
    bad.tokens.output = 40;
    assert!(sync::push(&s, request(vec![record("other"), bad])).is_err());
    assert_eq!(sync::changes(&s, 0, 500).unwrap().changes.len(), 1);
}
#[test]
fn forged_device_and_private_fields_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let s = Store::open(&tmp.path().join("s.db")).unwrap();
    assert!(sync::push(
        &s,
        PushRequest {
            protocol_version: 1,
            device_id: "wrong".into(),
            device_name: "Test".into(),
            records: vec![record("one")]
        }
    )
    .is_err());
    let mut wire = serde_json::to_value(record("one")).unwrap();
    wire["sourcePath"] = json!("private");
    assert!(serde_json::from_value::<UsageRecord>(wire).is_err());
}
#[test]
fn remote_pagination_and_device_views_do_not_duplicate_local() {
    let tmp = tempfile::tempdir().unwrap();
    let s = Store::open(&tmp.path().join("s.db")).unwrap();
    let own = s.device_id().unwrap();
    let mut r = record("one");
    r.device_id = own.clone();
    s.transaction(|| s.upsert(r.clone(), "fixture")).unwrap();
    let other = record("other");
    let response = ChangesResponse {
        server_instance_id: "server".into(),
        changes: vec![
            Change { seq: 1, record: r },
            Change {
                seq: 2,
                record: other,
            },
        ],
        next_cursor: 2,
        has_more: false,
        devices: vec![],
    };
    sync::apply_changes(&s, &response).unwrap();
    assert_eq!(s.records("all").unwrap().len(), 2);
    assert_eq!(s.records("local").unwrap().len(), 1);
    assert_eq!(s.records("test-device").unwrap().len(), 1);
    assert_eq!(s.meta::<i64>("download_cursor").unwrap(), Some(2));
}

fn legacy_db(path: &Path) {
    let c = rusqlite::Connection::open(path).unwrap();
    c.execute_batch("PRAGMA user_version=20;CREATE TABLE proxy_request_logs(request_id TEXT,app_type TEXT,model TEXT,pricing_model TEXT,input_tokens INTEGER,output_tokens INTEGER,cache_read_tokens INTEGER,cache_creation_tokens INTEGER,input_token_semantics INTEGER,total_cost_usd TEXT,created_at INTEGER,status_code INTEGER,data_source TEXT,session_id TEXT);CREATE TABLE usage_daily_rollups(date TEXT,app_type TEXT,provider_id TEXT,model TEXT,request_model TEXT,pricing_model TEXT,input_tokens INTEGER,output_tokens INTEGER,cache_read_tokens INTEGER,cache_creation_tokens INTEGER,input_token_semantics INTEGER,total_cost_usd TEXT,request_count INTEGER);INSERT INTO proxy_request_logs VALUES('old','codex','gpt-5.4',NULL,100,10,20,0,1,'0.50',1790812800,200,'codex_session','old-session');INSERT INTO usage_daily_rollups VALUES('2026-09-01','claude','session','claude-sonnet-4-5','','',1000,100,200,0,2,'2.50',3);").unwrap();
}
#[test]
fn migration_preview_commit_cutover_and_reimport() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("legacy.db");
    legacy_db(&path);
    let s = Store::open(&tmp.path().join("scope.db")).unwrap();
    let mut input = MigrationInput {
        path: path.to_string_lossy().into(),
        source_id: "old".into(),
        cutover_date: "2026-10-10".into(),
        timezone: "UTC".into(),
        expected_digest: None,
    };
    let p = migration::preview(&s, &input).unwrap();
    assert_eq!(p.records.len(), 2);
    assert_eq!(
        p.records
            .iter()
            .find(|r| r.grain == "event")
            .unwrap()
            .tokens
            .input,
        80
    );
    assert!(migration::commit(&s, &input).is_err());
    input.expected_digest = Some(p.digest);
    migration::commit(&s, &input).unwrap();
    migration::commit(&s, &input).unwrap();
    assert_eq!(s.local_records().unwrap().len(), 2);
    assert_eq!(s.pending_count().unwrap(), 2);
    assert_eq!(
        rusqlite::Connection::open(&path)
            .unwrap()
            .query_row("SELECT total_cost_usd FROM proxy_request_logs", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
        "0.50"
    );
}
#[test]
fn malformed_tail_and_outside_root_are_visible() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("bad.jsonl");
    fs::write(&p, "{\"type\":").unwrap();
    let (_, _, errors, complete) = files::read(&p).unwrap();
    assert!(!complete);
    assert_eq!(errors.len(), 1);
    let root = tmp.path().join("root");
    fs::create_dir(&root).unwrap();
    assert!(files::allowed_path(&root, &p).is_err());
}

#[test]
fn invalid_usage_does_not_silently_become_zero() {
    let mut lines = values(CLAUDE);
    lines[1]["message"]["usage"]["input_tokens"] = json!(-1);
    lines[2]["message"]["usage"]["output_tokens"] = json!("not-a-number");
    let parsed = Claude
        .parse(Path::new("fixture"), &lines, &TimelineIndex::new())
        .unwrap();
    assert!(!parsed.complete);
    assert!(parsed.usage.is_empty());
    assert_eq!(parsed.diagnostics.len(), 2);
}
#[test]
fn compressed_rollout_has_the_same_usage() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("session.jsonl.zst");
    fs::write(
        &path,
        zstd::stream::encode_all(CODEX.as_bytes(), 1).unwrap(),
    )
    .unwrap();
    assert_eq!(files::discover(tmp.path()).unwrap(), vec![path.clone()]);
    let (lines, _, errors, complete) = files::read(&path).unwrap();
    assert!(complete && errors.is_empty());
    assert_eq!(
        Codex
            .parse(&path, &lines, &TimelineIndex::new())
            .unwrap()
            .usage
            .iter()
            .map(|r| r.tokens.total())
            .sum::<u64>(),
        190
    );
}
#[test]
fn transcript_dates_are_milliseconds_and_inline_images_remain_local() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("logs");
    fs::create_dir(&root).unwrap();
    let path = root.join("s.jsonl");
    let image = json!({"type":"user","sessionId":"claude-fixture","timestamp":"2026-10-16T01:00:00Z","message":{"content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBORw0KGgo="}}]}});
    fs::write(&path, format!("{CLAUDE}{image}\n")).unwrap();
    let e = configured_engine(tmp.path(), &root, "claude");
    let sessions = e.sessions().unwrap();
    assert!(sessions[0].created_at.unwrap() > 1_000_000_000_000);
    let messages = e
        .execute(
            "session_messages",
            json!({"providerId":"claude","sourcePath":path}),
        )
        .unwrap();
    let image = &messages.as_array().unwrap().last().unwrap()["blocks"][0]["image"];
    let url = e
        .execute(
            "session_image",
            json!({"providerId":"claude","sourcePath":path,"image":image}),
        )
        .unwrap();
    assert_eq!(url, "data:image/png;base64,iVBORw0KGgo=");
    e.scan(false).unwrap();
    assert!(
        !serde_json::to_string(&e.lock().unwrap().pending().unwrap())
            .unwrap()
            .contains("iVBOR")
    );
}
#[test]
fn deleting_a_session_collects_and_retains_its_usage() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("logs");
    fs::create_dir(&root).unwrap();
    let path = root.join("s.jsonl");
    fs::write(&path, CLAUDE).unwrap();
    let modified = std::time::SystemTime::now() - std::time::Duration::from_secs(120);
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    let e = configured_engine(tmp.path(), &root, "claude");
    e.execute(
        "delete_session",
        json!({"providerId":"claude","sourcePath":path,"sessionId":"claude-fixture"}),
    )
    .unwrap();
    assert!(!path.exists());
    assert_eq!(e.lock().unwrap().records("local").unwrap().len(), 1);
    assert_eq!(e.lock().unwrap().pending_count().unwrap(), 1);
}

/// A third-party native SQLite source exercises the extension contract end-to-end.
#[test]
fn a_new_sqlite_harness_needs_no_storage_or_protocol_changes() {
    use harness_scope_core::harness::{Descriptor, ParseResult, Registry};
    struct SqliteHarness;
    impl HarnessAdapter for SqliteHarness {
        fn descriptor(&self) -> Descriptor {
            Descriptor {
                id: "sqlite-fixture",
                name: "SQLite fixture",
                parser_version: "v1",
                default_roots: vec![],
                usage: true,
                transcript: true,
                delete: false,
            }
        }
        fn discover(&self, root: &Path) -> anyhow::Result<Vec<std::path::PathBuf>> {
            Ok(vec![root.join("native.sqlite")])
        }
        fn read_source(
            &self,
            path: &Path,
        ) -> anyhow::Result<(Vec<Value>, String, Vec<String>, bool)> {
            let c = rusqlite::Connection::open(path)?;
            let mut stmt = c.prepare("SELECT id,body FROM native_events ORDER BY id")?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(json!({"id":row.get::<_,String>(0)?,"body":row.get::<_,String>(1)?}))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let fingerprint = files::hash(serde_json::to_string(&rows)?.as_bytes());
            Ok((rows, fingerprint, vec![], true))
        }
        fn parse(
            &self,
            _: &Path,
            events: &[Value],
            _: &TimelineIndex,
        ) -> anyhow::Result<ParseResult> {
            Ok(ParseResult {
                complete: true,
                diagnostics: vec![],
                usage: events
                    .iter()
                    .map(|e| ParsedUsage {
                        request_metadata: None,
                        provider: None,
                        id: format!("sqlite:{}", e["id"].as_str().unwrap()),
                        session_id: e["id"].as_str().unwrap().into(),
                        native_event_id: e["id"].as_str().unwrap().into(),
                        occurred_at: 1792112400,
                        model: "gpt-5.4".into(),
                        tokens: Tokens {
                            input: 10,
                            output: 1,
                            ..Default::default()
                        },
                        reported_cost: None,
                        timing: None,
                    })
                    .collect(),
            })
        }
        fn session(&self, _: &Path, _: &[Value]) -> Option<Session> {
            None
        }
        fn sessions(&self, path: &Path, events: &[Value]) -> Vec<Session> {
            events
                .iter()
                .map(|e| Session {
                    provider_id: "sqlite-fixture".into(),
                    session_id: e["id"].as_str().unwrap().into(),
                    title: Some(e["id"].as_str().unwrap().into()),
                    summary: None,
                    project_dir: None,
                    created_at: None,
                    last_active_at: None,
                    source_path: Some(path.to_string_lossy().into()),
                    resume_command: None,
                    can_delete: false,
                })
                .collect()
        }
        fn messages(&self, _: &[Value]) -> Vec<Value> {
            vec![]
        }
        fn messages_for_session(&self, events: &[Value], id: Option<&str>) -> Vec<Value> {
            events
                .iter()
                .filter(|e| e["id"].as_str() == id)
                .map(|e| json!({"role":"assistant","content":e["body"]}))
                .collect()
        }
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("native");
    fs::create_dir(&root).unwrap();
    let path = root.join("native.sqlite");
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch("CREATE TABLE native_events(id TEXT,body TEXT);INSERT INTO native_events VALUES('one','first transcript'),('two','second transcript');").unwrap();
    drop(c);
    let mut e = Engine::open(&tmp.path().join("scope.db")).unwrap();
    e.registry = Registry::new(vec![std::sync::Arc::new(SqliteHarness)]);
    let s = e.lock().unwrap();
    let mut cfg = s.config().unwrap();
    cfg.sources = vec![Source {
        id: "native".into(),
        harness: "sqlite-fixture".into(),
        root: root.to_string_lossy().into(),
        enabled: true,
    }];
    s.set_meta("config", &cfg).unwrap();
    drop(s);
    assert_eq!(e.scan(false).unwrap().imported, 2);
    assert_eq!(e.sessions().unwrap().len(), 2);
    assert_eq!(
        e.execute(
            "session_messages",
            json!({"providerId":"sqlite-fixture","sourcePath":path,"sessionId":"two"})
        )
        .unwrap()[0]["content"],
        "second transcript"
    );
    assert_eq!(e.scan(true).unwrap().updated, 0);
    assert_eq!(e.lock().unwrap().pending_count().unwrap(), 2);
}
