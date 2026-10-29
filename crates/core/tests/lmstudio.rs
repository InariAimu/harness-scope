use harness_scope_core::{
    harness::{lmstudio::LmStudio, HarnessAdapter, TimelineIndex},
    model::{PushRequest, Source},
    pricing,
    store::Store,
    sync, Engine,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/lmstudio/1793000000000.conversation.json"
    ))
    .unwrap()
}
fn write(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}
fn parse(path: &Path) -> harness_scope_core::harness::ParseResult {
    LmStudio
        .parse(
            path,
            &LmStudio.read_usage_source(path).unwrap().0,
            &TimelineIndex::new(),
        )
        .unwrap()
}
fn configure(engine: &Engine, root: &Path) {
    let store = engine.lock().unwrap();
    let mut config = store.config().unwrap();
    config.sources = vec![Source {
        id: "lmstudio-fixture".into(),
        harness: "lmstudio".into(),
        root: root.to_string_lossy().into(),
        enabled: true,
    }];
    config.server_url.clear();
    config.server_token.clear();
    store.set_meta("config", &config).unwrap();
}
fn generation(value: &mut Value) -> &mut Value {
    &mut value["messages"][1]["versions"][1]["steps"][0]
}

#[test]
fn reads_versions_and_native_totals_without_caching_private_content() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chat.conversation.json");
    write(&path, &fixture());
    let original = std::fs::read(&path).unwrap();
    let sessions = LmStudio.read_metadata(&path).unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].title.as_deref(), Some("Synthetic local chat"));
    assert!(!sessions[0].can_delete);
    assert!(sessions[0].resume_command.is_none());
    let messages = LmStudio.read_messages(&path, None).unwrap();
    assert_eq!(messages.len(), 4);
    assert_eq!(
        messages[1]["blocks"][1]["text"],
        "[Attachment: image/png · 42 bytes]"
    );
    assert_eq!(messages[2]["versionSelected"], false);
    assert_eq!(messages[3]["versionSelected"], true);
    assert_eq!(messages[2]["blocks"][0]["type"], "thinking");
    let (projected, _, _, _) = LmStudio.read_usage_source(&path).unwrap();
    let serialized = serde_json::to_string(&projected).unwrap();
    assert!(
        !serialized.contains("Private fixture")
            && !serialized.contains("must not be cached")
            && !serialized.contains("fixture-file")
    );
    let parsed = parse(&path);
    assert!(parsed.complete);
    assert!(parsed.diagnostics.is_empty());
    assert_eq!(parsed.usage.len(), 2);
    assert_eq!(
        parsed.usage.iter().map(|u| u.tokens.total()).sum::<u64>(),
        42
    );
    assert_eq!(parsed.usage[0].timing.as_ref().unwrap().duration_ms, 1200);
    assert_eq!(
        parsed.usage[0].timing.as_ref().unwrap().first_token_ms,
        Some(200)
    );
    assert!(parsed
        .usage
        .iter()
        .all(|u| u.reported_cost.is_none() && u.provider.is_none()));
    assert_eq!(original, std::fs::read(&path).unwrap());
    assert!(LmStudio.delete_source(&path).is_err());
}

#[test]
fn discovers_only_desktop_conversations_and_ignores_server_api_and_temp_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for name in [
        "conversations/folder/chat.conversation.json",
        "conversations/unfinished.conversation.json.tmp",
        "server-logs/api.conversation.json",
        ".internal/api-prediction-history/response.conversation.json",
    ] {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        write(&path, &fixture());
    }
    let paths = LmStudio.discover(root).unwrap();
    assert_eq!(paths.len(), 1);
    assert!(paths[0].ends_with("folder/chat.conversation.json"));
    assert!(LmStudio
        .discover(&root.join("server-logs"))
        .unwrap()
        .is_empty());
    assert!(LmStudio
        .read_source(&root.join("server-logs/api.conversation.json"))
        .is_err());
    assert!(LmStudio
        .discover(&root.join(".internal/api-prediction-history/response.conversation.json"))
        .is_err());
}

#[test]
fn incremental_rewrites_retries_and_copies_do_not_double_count_or_revoke_on_failure() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chat.conversation.json");
    let mut value = fixture();
    write(&path, &value);
    let engine = Engine::open(&dir.path().join("scope.db")).unwrap();
    configure(&engine, dir.path());
    let first = engine.scan(false).unwrap();
    assert_eq!(first.imported, 2);
    assert!(first.errors.is_empty());
    let repeat = engine.scan(false).unwrap();
    assert_eq!((repeat.imported, repeat.updated), (0, 0));
    let mut next = value["messages"][1]["versions"][1].clone();
    next["steps"][0]["stepIdentifier"] = json!("1793000006000-0.4");
    value["messages"][1]["versions"]
        .as_array_mut()
        .unwrap()
        .push(next);
    value["messages"][1]["currentlySelected"] = json!(2);
    write(&path, &value);
    let added = engine.scan(false).unwrap();
    assert_eq!(added.imported, 1);
    assert_eq!(added.updated, 0);
    let before = engine.lock().unwrap().records("local").unwrap();
    std::fs::write(&path, b"{\"messages\":[").unwrap();
    assert_eq!(engine.scan(false).unwrap().deferred_files, 1);
    assert_eq!(engine.lock().unwrap().records("local").unwrap(), before);
    let mut invalid = value.clone();
    generation(&mut invalid)["genInfo"]["stats"]["totalTokensCount"] = json!(999);
    write(&path, &invalid);
    assert_eq!(engine.scan(false).unwrap().deferred_files, 1);
    assert_eq!(engine.lock().unwrap().records("local").unwrap(), before);
    write(&path, &value);
    assert_eq!(engine.scan(false).unwrap().deferred_files, 0);
    std::fs::copy(&path, dir.path().join("copy.conversation.json")).unwrap();
    let copied = engine.scan(true).unwrap();
    assert_eq!((copied.imported, copied.updated), (0, 0));
    let mut duplicate = value.clone();
    duplicate["createdAt"] = json!(1793000100000_i64);
    write(&dir.path().join("copy.conversation.json"), &duplicate);
    let duplicated_chat = engine.scan(true).unwrap();
    assert_eq!((duplicated_chat.imported, duplicated_chat.updated), (0, 0));
    assert!(engine
        .lock()
        .unwrap()
        .records("local")
        .unwrap()
        .iter()
        .all(|r| r.session_id.as_deref() == Some("conversation:1793000000000")));
    value["messages"][1]["currentlySelected"] = json!(0);
    write(&path, &value);
    assert_eq!(engine.scan(false).unwrap().updated, 0);
    let cache: String = engine
        .lock()
        .unwrap()
        .conn
        .query_row("SELECT payload FROM usage_read_cache LIMIT 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(!cache.contains("Private fixture"));
    assert_eq!(engine.lock().unwrap().records("local").unwrap().len(), 3);
}

#[test]
fn stays_unpriced_with_matching_prices_and_syncs_facts_without_new_timing_contract() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chat.conversation.json");
    write(&path, &fixture());
    let engine = Engine::open(&dir.path().join("scope.db")).unwrap();
    configure(&engine, &path);
    let mut price = pricing::builtin().remove(0);
    price.model_id = "local/fixture-model".into();
    engine
        .lock()
        .unwrap()
        .set_meta("prices", &vec![price])
        .unwrap();
    assert_eq!(engine.scan(false).unwrap().imported, 2);
    let summary = engine
        .execute(
            "usage_query",
            json!({"kind":"summary","appType":"lmstudio"}),
        )
        .unwrap();
    assert_eq!(summary["totalRequests"], 2);
    assert_eq!(summary["realTotalTokens"], 42);
    assert_eq!(summary["cacheBreakdownKnown"], false);
    assert_eq!(
        engine
            .execute("reprice_usage", json!({"harness":"lmstudio"}))
            .unwrap(),
        0
    );
    assert!(engine
        .execute("models_needing_prices", json!({}))
        .unwrap()
        .as_array()
        .unwrap()
        .is_empty());
    let store = engine.lock().unwrap();
    let records = store.records("local").unwrap();
    assert!(records
        .iter()
        .all(|r| r.cost.amount_usd.is_none() && r.cost.method == "unknown" && r.timing.is_none()));
    assert_eq!(store.timings().unwrap().len(), 2);
    let pending = store.pending().unwrap();
    drop(store);
    let wire = serde_json::to_string(&pending).unwrap();
    assert!(!wire.contains("Private fixture") && !wire.contains("prediction_stats"));
    let server = Store::open(&dir.path().join("server.db")).unwrap();
    let remote = Store::open(&dir.path().join("remote.db")).unwrap();
    sync::push(
        &server,
        PushRequest {
            protocol_version: 4,
            device_id: pending[0].device_id.clone(),
            device_name: "fixture".into(),
            records: pending,
        },
    )
    .unwrap();
    sync::apply_changes(&remote, &sync::changes(&server, 0, 100).unwrap()).unwrap();
    assert_eq!(remote.records("all").unwrap().len(), 2);
    assert_eq!(engine.scan(true).unwrap().updated, 0);
    assert_eq!(engine.lock().unwrap().timings().unwrap().len(), 2);
}

#[test]
fn rejects_invalid_counters_identity_and_shapes_but_accepts_true_zero() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chat.conversation.json");
    for (key, bad) in [
        ("promptTokensCount", json!(-1)),
        ("predictedTokensCount", Value::Null),
        ("totalTokensCount", json!(1.5)),
    ] {
        let mut value = fixture();
        generation(&mut value)["genInfo"]["stats"][key] = bad;
        write(&path, &value);
        let parsed = parse(&path);
        assert!(!parsed.complete);
        assert_eq!(parsed.usage.len(), 1);
    }
    let mut value = fixture();
    generation(&mut value)["stepIdentifier"] = json!("unknown-id");
    write(&path, &value);
    assert!(!parse(&path).complete);
    let mut value = fixture();
    generation(&mut value)
        .as_object_mut()
        .unwrap()
        .remove("genInfo");
    write(&path, &value);
    assert!(!parse(&path).complete);
    let mut value = fixture();
    value["messages"][1]["currentlySelected"] = json!(999);
    write(&path, &value);
    assert!(LmStudio.read_source(&path).is_err());
    let mut value = fixture();
    for key in [
        "promptTokensCount",
        "predictedTokensCount",
        "totalTokensCount",
    ] {
        generation(&mut value)["genInfo"]["stats"][key] = json!(0)
    }
    write(&path, &value);
    assert!(parse(&path).complete);
}

#[test]
fn source_initializes_once_and_respects_disabled_or_removed_settings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scope.db");
    let store = Store::open(&path).unwrap();
    let mut config = store.config().unwrap();
    config.sources = vec![Source {
        id: "kept".into(),
        harness: "codex".into(),
        root: "synthetic".into(),
        enabled: false,
    }];
    store.set_meta("config", &config).unwrap();
    drop(store);
    let engine = Engine::open(&path).unwrap();
    let store = engine.lock().unwrap();
    let mut config = store.config().unwrap();
    assert_eq!(
        config
            .sources
            .iter()
            .filter(|s| s.harness == "lmstudio")
            .count(),
        1
    );
    config
        .sources
        .iter_mut()
        .find(|s| s.harness == "lmstudio")
        .unwrap()
        .enabled = false;
    store.set_meta("config", &config).unwrap();
    drop(store);
    drop(engine);
    let engine = Engine::open(&path).unwrap();
    let store = engine.lock().unwrap();
    let mut config = store.config().unwrap();
    assert!(
        !config
            .sources
            .iter()
            .find(|s| s.harness == "lmstudio")
            .unwrap()
            .enabled
    );
    config.sources.retain(|s| s.harness != "lmstudio");
    store.set_meta("config", &config).unwrap();
    drop(store);
    drop(engine);
    assert!(!Engine::open(&path)
        .unwrap()
        .lock()
        .unwrap()
        .config()
        .unwrap()
        .sources
        .iter()
        .any(|s| s.harness == "lmstudio"));
}

#[test]
#[ignore = "Read-only inspection of an explicitly selected local LM Studio installation"]
fn inspect_local_lmstudio() {
    let root = PathBuf::from(
        std::env::var("HARNESS_SCOPE_LMSTUDIO_SOURCE").expect("Set source explicitly"),
    );
    let db = PathBuf::from(
        std::env::var("HARNESS_SCOPE_LMSTUDIO_VERIFY_DB").expect("Set isolated output database"),
    );
    let allowed = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../output/lmstudio-20261028")
        .canonicalize()
        .unwrap();
    assert!(
        db.parent()
            .unwrap()
            .canonicalize()
            .unwrap()
            .starts_with(allowed)
            && !db.exists()
    );
    let paths = LmStudio.discover(&root).unwrap();
    let hashes: Vec<_> = paths
        .iter()
        .map(|p| LmStudio.read_usage_source(p).unwrap().1)
        .collect();
    let engine = Engine::open(&db).unwrap();
    configure(&engine, &root);
    let scan = engine.scan(false).unwrap();
    assert!(scan.errors.is_empty());
    assert_eq!(scan.deferred_files, 0);
    let sessions = engine.sessions().unwrap();
    assert_eq!(sessions.len(), paths.len());
    for session in &sessions {
        let messages = LmStudio
            .read_messages(
                Path::new(session.source_path.as_ref().unwrap()),
                Some(&session.session_id),
            )
            .unwrap();
        assert!(!messages.is_empty());
    }
    let records = engine.lock().unwrap().records("local").unwrap();
    assert!(records.iter().all(|r| r.cost.amount_usd.is_none()));
    let repeat = engine.scan(false).unwrap();
    assert_eq!((repeat.imported, repeat.updated), (0, 0));
    assert_eq!(
        hashes,
        paths
            .iter()
            .map(|p| LmStudio.read_usage_source(p).unwrap().1)
            .collect::<Vec<_>>()
    );
    println!(
        "{}",
        json!({"sessions":sessions.len(),"generations":records.len(),"inputTokens":records.iter().map(|r|r.tokens.input).sum::<u64>(),"outputTokens":records.iter().map(|r|r.tokens.output).sum::<u64>(),"unpriced":records.len(),"repeatImported":repeat.imported,"repeatUpdated":repeat.updated,"sourceUnchanged":true})
    );
}
