use harness_scope_core::{
    harness::{antigravity::Antigravity, gemini::Gemini, HarnessAdapter, TimelineIndex},
    model::{PushRequest, Source},
    store::Store,
    sync, Engine,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::path::Path;

const JSON: &str = include_str!("../../../tests/fixtures/gemini/session-native.json");
const JSONL: &str = include_str!("../../../tests/fixtures/gemini/session-native.jsonl");
fn parsed(adapter: &dyn HarnessAdapter, path: &Path) -> harness_scope_core::harness::ParseResult {
    adapter
        .parse(
            path,
            &adapter.read_usage_source(path).unwrap().0,
            &TimelineIndex::new(),
        )
        .unwrap()
}
fn configure(engine: &Engine, sources: Vec<(&str, &Path)>) {
    let store = engine.lock().unwrap();
    let mut config = store.config().unwrap();
    config.sources = sources
        .into_iter()
        .map(|(h, p)| Source {
            id: h.into(),
            harness: h.into(),
            root: p.to_string_lossy().into(),
            enabled: true,
        })
        .collect();
    config.server_url.clear();
    config.server_token.clear();
    store.set_meta("config", &config).unwrap();
}

fn varint(mut n: u64) -> Vec<u8> {
    let mut out = Vec::new();
    while n >= 128 {
        out.push((n as u8 & 127) | 128);
        n >>= 7;
    }
    out.push(n as u8);
    out
}
fn number(field: u32, n: u64) -> Vec<u8> {
    [varint(u64::from(field) << 3), varint(n)].concat()
}
fn bytes(field: u32, b: &[u8]) -> Vec<u8> {
    [
        varint((u64::from(field) << 3) | 2),
        varint(b.len() as u64),
        b.to_vec(),
    ]
    .concat()
}
fn text(field: u32, s: &str) -> Vec<u8> {
    bytes(field, s.as_bytes())
}
fn metadata(at: u64, origin: &str) -> Vec<u8> {
    [bytes(1, &number(1, at)), bytes(20, &text(4, origin))].concat()
}
fn generation(model: &str, id: &str) -> Vec<u8> {
    generation_with_durations(model, id, 30, Some(number(1, 1)), Some(number(1, 2)))
}
fn generation_with_durations(
    model: &str,
    id: &str,
    output: u64,
    first: Option<Vec<u8>>,
    stream: Option<Vec<u8>>,
) -> Vec<u8> {
    let usage = [
        number(2, 100),
        number(3, output),
        number(5, 400),
        number(9, 10),
        number(10, output.saturating_sub(10)),
        text(7, id),
    ]
    .concat();
    let mut chat = [
        bytes(4, &usage),
        text(22, model),
        text(1, "Private Antigravity system prompt"),
    ]
    .concat();
    if let Some(first) = first {
        chat.extend(bytes(11, &first));
    }
    if let Some(stream) = stream {
        chat.extend(bytes(12, &stream));
    }
    [
        bytes(1, &chat),
        bytes(2, &[1, 2]),
        text(4, "synthetic-execution"),
    ]
    .concat()
}
fn native_db(path: &Path, session: &str) -> Connection {
    let db = Connection::open(path).unwrap();
    db.execute_batch(include_str!(
        "../../../tests/fixtures/antigravity/schema.sql"
    ))
    .unwrap();
    db.execute("INSERT INTO trajectory_meta VALUES(?1,?1,0,0)", [session])
        .unwrap();
    let meta = metadata(1795050001, session);
    let user = [
        number(1, 14),
        bytes(19, &text(2, "Private synthetic Antigravity user input")),
    ]
    .concat();
    let response = [
        number(1, 15),
        bytes(
            20,
            &[
                text(1, "Private synthetic Antigravity response"),
                text(3, "Private synthetic thinking"),
            ]
            .concat(),
        ),
    ]
    .concat();
    let tool = [
        bytes(
            4,
            &[
                text(1, "tool-1"),
                text(2, "read_file"),
                text(3, "{\"path\":\"private-file\"}"),
            ]
            .concat(),
        ),
        meta.clone(),
    ]
    .concat();
    for (idx, m, payload) in [
        (0, meta.clone(), user),
        (1, meta.clone(), response),
        (2, tool, vec![]),
    ] {
        db.execute(
            "INSERT INTO steps(idx,metadata,step_payload) VALUES(?1,?2,?3)",
            params![idx, m, payload],
        )
        .unwrap();
    }
    let gen = generation("gemini-4-neon", "native-request-1");
    db.execute(
        "INSERT INTO gen_metadata(idx,data,size) VALUES(0,?1,?2)",
        params![gen, gen.len()],
    )
    .unwrap();
    db
}

#[test]
fn gemini_json_reads_native_tokens_transcript_and_private_cache_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session-native.json");
    std::fs::write(&path, JSON).unwrap();
    let before = std::fs::read(&path).unwrap();
    let p = parsed(&Gemini, &path);
    assert!(p.complete && p.diagnostics.is_empty());
    assert_eq!(p.usage.len(), 1);
    let u = &p.usage[0];
    assert_eq!(
        (
            u.tokens.input,
            u.tokens.output,
            u.tokens.cache_read,
            u.tokens.reasoning
        ),
        (65, 30, 40, 10)
    );
    assert_eq!(u.tokens.total(), 135);
    assert!(u.provider.is_none());
    assert_eq!(u.model, "gemini-2.5-pro");
    assert_eq!(u.occurred_at, 1795050001);
    let sessions = Gemini.read_metadata(&path).unwrap();
    assert_eq!(
        sessions[0].project_dir.as_deref(),
        Some("/synthetic/workspace")
    );
    assert!(!sessions[0].can_delete);
    let messages = Gemini.read_messages(&path, None).unwrap();
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["blocks"][0]["type"], "thinking");
    assert_eq!(messages[1]["blocks"][2]["type"], "tool_call");
    assert_eq!(messages[1]["blocks"][3]["type"], "tool_result");
    let cache = serde_json::to_string(&Gemini.read_usage_source(&path).unwrap().0).unwrap();
    for private in ["Private", "private-file", "thought body", "workspace"] {
        assert!(!cache.contains(private));
    }
    assert_eq!(before, std::fs::read(&path).unwrap());
    assert!(Gemini.delete_source(&path).is_err());
}

#[test]
fn jsonl_replays_do_not_double_count_and_rewind_preserves_incurred_usage() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session-native.jsonl");
    std::fs::write(&path, JSONL).unwrap();
    let p = parsed(&Gemini, &path);
    assert!(p.complete);
    assert_eq!(p.usage.len(), 2);
    assert_eq!(p.usage.iter().map(|u| u.tokens.total()).sum::<u64>(), 147);
    let messages = Gemini.read_messages(&path, None).unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1]["id"], "call-2");
    assert_eq!(
        Gemini.read_metadata(&path).unwrap()[0].title.as_deref(),
        Some("Synthetic Gemini session")
    );
    std::fs::write(path.with_extension("json"), JSON).unwrap();
    assert_eq!(Gemini.discover(dir.path()).unwrap(), vec![path]);
}

#[test]
fn gemini_invalid_missing_and_negative_tokens_never_become_confirmed_zero() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session-native.json");
    for (key, bad) in [
        ("input", json!(-1)),
        ("output", json!(null)),
        ("thoughts", json!("10")),
        ("cached", json!(200)),
        ("total", json!(99)),
    ] {
        let mut v: Value = serde_json::from_str(JSON).unwrap();
        v["messages"][1]["tokens"][key] = bad;
        std::fs::write(&path, v.to_string()).unwrap();
        let p = parsed(&Gemini, &path);
        assert!(!p.complete);
        assert!(p.usage.is_empty());
        assert!(!p.diagnostics.is_empty());
    }
    let mut v: Value = serde_json::from_str(JSON).unwrap();
    v["messages"][1]["tokens"] = Value::Null;
    std::fs::write(&path, v.to_string()).unwrap();
    assert!(!parsed(&Gemini, &path).complete);
    v["messages"][1]["tokens"] = json!({"input":0,"output":0,"cached":0,"total":0});
    std::fs::write(&path, v.to_string()).unwrap();
    let p = parsed(&Gemini, &path);
    assert!(p.complete);
    assert_eq!(p.usage[0].tokens.total(), 0);
}

#[test]
fn corrupt_jsonl_tail_defers_without_retracting_previous_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session-native.jsonl");
    std::fs::write(&path, JSONL).unwrap();
    let engine = Engine::open(&dir.path().join("scope.db")).unwrap();
    configure(&engine, vec![("gemini", &path)]);
    assert_eq!(engine.scan(false).unwrap().imported, 2);
    std::fs::write(&path, format!("{JSONL}{{\"id\":")).unwrap();
    assert_eq!(engine.scan(false).unwrap().deferred_files, 1);
    assert_eq!(
        engine
            .lock()
            .unwrap()
            .records("local")
            .unwrap()
            .iter()
            .filter(|u| !u.deleted)
            .count(),
        2
    );
    assert!(Gemini.read_messages(&path, None).is_err());
    std::fs::write(&path, JSONL).unwrap();
    assert_eq!(engine.scan(false).unwrap().deferred_files, 0);
}

#[test]
fn antigravity_reads_one_generation_not_duplicate_step_usage_and_supports_wal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("conversation.db");
    let writer = native_db(&path, "antigravity-synthetic");
    writer.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
    let p = parsed(&Antigravity, &path);
    assert!(p.complete);
    assert_eq!(p.usage.len(), 1);
    assert_eq!(
        (
            p.usage[0].tokens.input,
            p.usage[0].tokens.output,
            p.usage[0].tokens.cache_read
        ),
        (100, 30, 400)
    );
    assert_eq!(p.usage[0].tokens.reasoning, 10);
    assert_eq!(p.usage[0].tokens.total(), 530);
    let timing = p.usage[0].timing.as_ref().unwrap();
    assert_eq!(timing.duration_ms, 3000);
    assert_eq!(timing.first_token_ms, Some(1000));
    assert_eq!(timing.duration_source, "antigravity_chat_model");
    assert!(!Antigravity.syncs_timing());
    let cache = serde_json::to_string(&Antigravity.read_usage_source(&path).unwrap().0).unwrap();
    assert!(!cache.contains("Private") && !cache.contains("private-file"));
    let messages = Antigravity.read_messages(&path, None).unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[1]["blocks"][0]["type"], "thinking");
    let gen = generation("gemini-2.5-pro", "native-request-2");
    writer
        .execute("INSERT INTO gen_metadata(idx,data) VALUES(1,?1)", [gen])
        .unwrap();
    assert_eq!(parsed(&Antigravity, &path).usage.len(), 2);
    assert!(!Antigravity.read_metadata(&path).unwrap()[0].can_delete);
    assert!(Antigravity.delete_source(&path).is_err());
}

#[test]
fn native_missing_usage_corruption_and_unknown_encoding_are_deferred() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("conversation.db");
    let db = native_db(&path, "antigravity-synthetic");
    db.execute(
        "UPDATE gen_metadata SET data=?1",
        [bytes(1, &text(22, "gemini-4-neon"))],
    )
    .unwrap();
    let (_, _, errors, complete) = Antigravity.read_usage_source(&path).unwrap();
    assert!(!complete && !errors.is_empty());
    assert!(parsed(&Antigravity, &path).usage.is_empty());
    db.execute("UPDATE gen_metadata SET data=?1", [vec![10u8, 128]])
        .unwrap();
    assert!(!Antigravity.read_usage_source(&path).unwrap().3);
    db.execute("UPDATE steps SET step_format=99", []).unwrap();
    assert!(Antigravity.read_usage_source(&path).is_err());
}

#[test]
fn forked_antigravity_generations_keep_native_origin_and_stable_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("conversation.db");
    let db = native_db(&path, "parent");
    let before = parsed(&Antigravity, &path).usage.remove(0);
    db.execute("UPDATE trajectory_meta SET cascade_id='child'", [])
        .unwrap();
    let after = parsed(&Antigravity, &path).usage.remove(0);
    assert_eq!(before.id, after.id);
    assert_eq!(after.session_id, "parent");
    db.execute(
        "INSERT INTO gen_metadata(idx,data) SELECT 1,data FROM gen_metadata WHERE idx=0",
        [],
    )
    .unwrap();
    assert_eq!(parsed(&Antigravity, &path).usage.len(), 1);
}

#[test]
fn discovery_excludes_summaries_and_does_not_silently_accept_legacy_protobuf() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("conversations");
    std::fs::create_dir(&root).unwrap();
    let path = root.join("native.db");
    let _db = native_db(&path, "test");
    std::fs::write(
        root.join("conversation_summaries.db"),
        b"not a conversation",
    )
    .unwrap();
    std::fs::write(root.join("native.db-wal"), b"sidecar").unwrap();
    assert_eq!(
        Antigravity.discover(dir.path()).unwrap(),
        vec![path.clone()]
    );
    std::fs::write(root.join("legacy.pb"), b"unsupported").unwrap();
    assert_eq!(Antigravity.discover(dir.path()).unwrap(), vec![path]);
    assert!(Antigravity.discover(&root.join("legacy.pb")).is_err());
    let legacy = dir.path().join("legacy-only");
    std::fs::create_dir(&legacy).unwrap();
    std::fs::write(legacy.join("old.pb"), b"unsupported").unwrap();
    assert!(Antigravity.discover(&legacy).is_err());
}

#[test]
fn scans_are_idempotent_sync_safe_and_google_category_links_unknown_session_routes() {
    let dir = tempfile::tempdir().unwrap();
    let gemini = dir.path().join("session-native.json");
    std::fs::write(&gemini, JSON).unwrap();
    let antigravity = dir.path().join("native.db");
    let _native = native_db(&antigravity, "antigravity-synthetic");
    let engine = Engine::open(&dir.path().join("scope.db")).unwrap();
    configure(
        &engine,
        vec![("gemini", &gemini), ("antigravity", &antigravity)],
    );
    assert_eq!(engine.scan(false).unwrap().imported, 2);
    assert_eq!(engine.scan(true).unwrap().updated, 0);
    let routes = engine
        .execute("provider_account_routes", json!({}))
        .unwrap();
    assert!(routes
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["provider"] == "gemini (Session)"));
    let account=engine.execute("provider_account_save",json!({"provider":"google","name":"Google AI","links":[{"harness":"gemini","provider":"gemini (Session)"},{"harness":"antigravity","provider":"antigravity (Session)"}]})).unwrap();
    assert_eq!(account["canRefresh"], false);
    assert_eq!(account["keyConfigured"], false);
    assert!(engine
        .execute(
            "provider_account_refresh",
            json!({"id":account["account"]["id"]})
        )
        .is_err());
    assert!(engine
        .execute(
            "provider_account_save",
            json!({"provider":"google","name":"Google","apiKey":"not-a-google-quota-key"})
        )
        .is_err());
    let usage = engine
        .execute(
            "provider_account_usage",
            json!({"id":account["account"]["id"],"range":"subscription"}),
        )
        .unwrap();
    assert_eq!(usage["summary"]["totalRequests"], 2);
    assert_eq!(usage["summary"]["realTotalTokens"], 665);
    let records = engine.lock().unwrap().records("local").unwrap();
    let neon = records.iter().find(|r| r.harness == "antigravity").unwrap();
    assert_eq!(neon.model, "gemini-4-neon");
    assert!(neon.cost.amount_usd.is_none());
    let g = records.iter().find(|r| r.harness == "gemini").unwrap();
    if g.cost.method == "estimated" {
        assert_eq!(g.cost.billing_basis.as_deref(), Some("model_reference"));
    }
    let payload = serde_json::to_string(&records).unwrap();
    assert!(!payload.contains("Private") && !payload.contains("workspace"));
    let remote = Store::open(&dir.path().join("server.db")).unwrap();
    sync::push(
        &remote,
        PushRequest {
            protocol_version: 4,
            device_id: engine.lock().unwrap().device_id().unwrap(),
            device_name: "synthetic".into(),
            records,
        },
    )
    .unwrap();
    let peer = Store::open(&dir.path().join("peer.db")).unwrap();
    sync::apply_changes(&peer, &sync::changes(&remote, 0, 100).unwrap()).unwrap();
    assert_eq!(
        peer.records("all")
            .unwrap()
            .iter()
            .filter(|r| !r.deleted)
            .count(),
        2
    );
    let cache: String = engine
        .lock()
        .unwrap()
        .conn
        .query_row(
            "SELECT group_concat(payload) FROM usage_read_cache",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!cache.contains("Private") && !cache.contains("private-file"));
}

#[test]
fn existing_installs_add_sources_once_and_server_does_not_initialize_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scope.db");
    {
        let store = Store::open(&path).unwrap();
        let mut config = store.config().unwrap();
        config.sources = vec![Source {
            id: "existing".into(),
            harness: "codex".into(),
            root: "/synthetic/disabled".into(),
            enabled: false,
        }];
        store.set_meta("config", &config).unwrap();
    }
    let engine = Engine::open(&path).unwrap();
    {
        let s = engine.lock().unwrap();
        let mut c = s.config().unwrap();
        for harness in ["gemini", "antigravity"] {
            assert_eq!(c.sources.iter().filter(|s| s.harness == harness).count(), 1);
        }
        c.sources
            .retain(|s| !matches!(s.harness.as_str(), "gemini" | "antigravity"));
        s.set_meta("config", &c).unwrap();
    }
    drop(engine);
    let engine = Engine::open(&path).unwrap();
    assert!(!engine
        .lock()
        .unwrap()
        .config()
        .unwrap()
        .sources
        .iter()
        .any(|s| matches!(s.harness.as_str(), "gemini" | "antigravity")));
    let server = Engine::open_sync_server(&dir.path().join("server.db")).unwrap();
    assert!(server.lock().unwrap().config().unwrap().sources.is_empty());
}

#[test]
fn native_timing_backfills_without_changing_usage_ids_and_stays_local_for_old_servers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("native.db");
    let writer = native_db(&path, "antigravity-timing");
    writer
        .execute(
            "UPDATE gen_metadata SET data=?1",
            [generation_with_durations(
                "gemini-4-neon",
                "native-request-1",
                400,
                None,
                None,
            )],
        )
        .unwrap();
    let engine = Engine::open(&dir.path().join("scope.db")).unwrap();
    configure(&engine, vec![("antigravity", &path)]);
    assert_eq!(engine.scan(false).unwrap().imported, 1);
    let before = engine.lock().unwrap().records("local").unwrap().remove(0);
    assert!(engine.lock().unwrap().timings().unwrap().is_empty());
    writer
        .execute(
            "UPDATE gen_metadata SET data=?1",
            [generation_with_durations(
                "gemini-4-neon",
                "native-request-1",
                400,
                Some(number(1, 1)),
                Some(number(1, 2)),
            )],
        )
        .unwrap();
    assert_eq!(engine.scan(false).unwrap().imported, 0);
    let store = engine.lock().unwrap();
    let after = store.records("local").unwrap().remove(0);
    assert_eq!(before, after);
    assert_eq!(store.timings().unwrap().len(), 1);
    assert!(store.pending().unwrap().iter().all(|r| r.timing.is_none()));
    drop(store);
    let args = json!({"kind":"logs","startDate":1795040000,"endDate":1795060000,"filters":{"appType":"antigravity"}});
    let logs = engine.execute("usage_query", args).unwrap();
    let row = &logs["data"][0];
    assert_eq!(row["firstTokenMs"], 1000);
    assert_eq!(row["durationMs"], 3000);
    assert_eq!(row["timingMethod"], "reported");
    assert_eq!(row["estimatedSpeed"]["durationMs"], 2000);
    assert_eq!(row["estimatedSpeed"]["eligible"], true);
    let daily=engine.execute("usage_query",json!({"kind":"speedDaily","startDate":1795040000,"endDate":1795060000,"filters":{"appType":"antigravity"}})).unwrap();
    assert_eq!(daily["days"][0]["sampleCount"], 1);
    assert_eq!(daily["days"][0]["estimatedSampleCount"], 0);
    assert_eq!(engine.scan(false).unwrap().updated, 0);
}

#[test]
fn invalid_or_missing_native_timing_does_not_invent_zero_or_discard_valid_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("native.db");
    let writer = native_db(&path, "timing-unknown");
    for (first, stream) in [
        (None, Some(number(1, 2))),
        (Some(number(1, 1)), None),
        (Some(number(1, u64::MAX)), Some(number(1, 2))),
        (Some(number(2, 1_000_000_000)), Some(number(1, 2))),
        (Some(number(1, 3600)), Some(number(1, 2))),
        (Some(vec![8, 128]), Some(number(1, 2))),
    ] {
        writer
            .execute(
                "UPDATE gen_metadata SET data=?1",
                [generation_with_durations(
                    "gemini-4-neon",
                    "native-request-1",
                    400,
                    first,
                    stream,
                )],
            )
            .unwrap();
        let p = parsed(&Antigravity, &path);
        assert!(p.complete);
        assert_eq!(p.usage.len(), 1);
        assert!(p.usage[0].timing.is_none());
    }
    writer
        .execute(
            "UPDATE gen_metadata SET data=?1",
            [generation_with_durations(
                "gemini-4-neon",
                "native-request-1",
                400,
                Some(vec![]),
                Some(number(1, 2)),
            )],
        )
        .unwrap();
    assert_eq!(
        parsed(&Antigravity, &path).usage[0]
            .timing
            .as_ref()
            .unwrap()
            .first_token_ms,
        Some(0)
    );
}

#[test]
#[ignore = "Read-only opt-in probe of this machine's native Antigravity data"]
fn inspect_local_antigravity_read_only() {
    let root =
        std::env::var_os("HARNESS_SCOPE_ANTIGRAVITY_PROBE").expect("Set explicit probe directory");
    let mut count = 0;
    let mut total = 0;
    let mut timed = 0;
    for path in Antigravity.discover(Path::new(&root)).unwrap() {
        let (values, _, errors, complete) = Antigravity.read_usage_source(&path).unwrap();
        let p = Antigravity
            .parse(&path, &values, &TimelineIndex::new())
            .unwrap();
        assert!(complete, "{errors:?}");
        assert!(p.complete, "{:?}", p.diagnostics);
        count += p.usage.len();
        total += p.usage.iter().map(|u| u.tokens.total()).sum::<u64>();
        timed += p
            .usage
            .iter()
            .filter(|u| {
                u.timing
                    .as_ref()
                    .is_some_and(|t| t.first_token_ms.is_some())
            })
            .count();
        assert!(!Antigravity.read_metadata(&path).unwrap().is_empty());
        assert!(!Antigravity.read_messages(&path, None).unwrap().is_empty());
    }
    println!("Antigravity native read-only validation: {count} generations, {total} tokens, {timed} with native TTFT and streaming duration");
    assert!(count > 0);
    assert!(timed > 0);
}

#[test]
fn antigravity_wal_updates_cached_transcript_and_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("conversation.db");
    let writer = native_db(&path, "review-session");
    writer
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
        .unwrap();
    let engine = Engine::open(&dir.path().join("harness.db")).unwrap();
    configure(&engine, vec![("antigravity", &path)]);
    let args = json!({"providerId":"antigravity","sourcePath":path,"sessionId":"review-session"});
    let first = engine.execute("session_transcript", args.clone()).unwrap();
    assert_eq!(first["header"]["total"], 3);
    assert_eq!(
        engine.sessions().unwrap()[0].last_active_at,
        Some(1795050001000)
    );
    writer.execute("INSERT INTO steps(idx,metadata,step_payload) SELECT 3,?1,step_payload FROM steps WHERE idx=0", [metadata(1795050010, "review-session")]).unwrap();
    assert_eq!(
        engine.sessions().unwrap()[0].last_active_at,
        Some(1795050010000)
    );
    assert_eq!(Antigravity.read_messages(&path, None).unwrap().len(), 4);
    let fresh = engine.execute("session_transcript", args).unwrap();
    assert_eq!(fresh["header"]["total"], 4);
}

#[test]
fn gemini_partial_order_keeps_unlisted_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session-order.jsonl");
    let mut lines = vec![json!({"$set":{"sessionId":"test","startTime":"2026-10-01T00:00:00Z"}})];
    for id in ["A", "B", "C"] {
        lines.push(json!({"id":id,"type":"user","content":id,"timestamp":"2026-10-01T00:00:00Z"}));
    }
    lines.push(json!({"$patch":{"orderIds":["C","B"]}}));
    std::fs::write(
        &path,
        lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    let rows = Gemini.read_messages(&path, None).unwrap();
    let ids = rows
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(ids, ["A", "C", "B"]);
}
