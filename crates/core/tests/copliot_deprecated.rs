use harness_scope_core::{
    harness::{
        copliot_deprecated::{CopliotDeprecated, Selection, ID, PROVIDER, SELECTION_FILE},
        HarnessAdapter, TimelineIndex,
    },
    model::Source,
    Engine,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

const LEGACY: &str = include_str!("../../../tests/fixtures/copliot-deprecated/legacy.json");
const CONTINUED: &str = include_str!("../../../tests/fixtures/copliot-deprecated/continued.jsonl");
const MODERN: &str = include_str!("../../../tests/fixtures/copliot/modern.jsonl");

fn selection(root: &Path, ids: &[&str]) {
    fs::write(
        root.join(SELECTION_FILE),
        serde_json::to_vec(&Selection {
            version: 1,
            session_ids: ids.iter().map(|id| id.to_string()).collect::<BTreeSet<_>>(),
        })
        .unwrap(),
    )
    .unwrap();
}
fn source(root: &Path, id: &str, extension: &str, content: &str) -> PathBuf {
    let dir = root.join("workspaceStorage/project/chatSessions");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{id}.{extension}"));
    fs::write(&path, content).unwrap();
    path
}
fn parse(path: &Path) -> harness_scope_core::harness::ParseResult {
    let (events, _, errors, complete) = CopliotDeprecated.read_source(path).unwrap();
    assert!(complete && errors.is_empty());
    CopliotDeprecated
        .parse(path, &events, &TimelineIndex::new())
        .unwrap()
}
fn engine(tmp: &Path, root: &Path) -> Engine {
    let engine = Engine::open(&tmp.join("scope.db")).unwrap();
    let store = engine.lock().unwrap();
    let mut config = store.config().unwrap();
    config.sources = vec![Source {
        id: "legacy".into(),
        harness: ID.into(),
        root: root.to_string_lossy().into_owned(),
        enabled: true,
    }];
    store.set_meta("config", &config).unwrap();
    drop(store);
    engine
}

#[test]
fn legacy_zero_usage_requests_are_preserved_without_last_call_estimates() {
    let tmp = tempfile::tempdir().unwrap();
    let path = source(tmp.path(), "synthetic-copliot-legacy", "json", LEGACY);
    let result = parse(&path);
    assert!(result.complete);
    assert_eq!(result.usage.len(), 2);
    assert!(result
        .usage
        .iter()
        .all(|r| r.tokens.total() == 0 && r.provider.as_deref() == Some(PROVIDER)));
    assert!(result
        .usage
        .iter()
        .all(|r| r.reported_cost.is_none() && r.timing.is_none()));
    assert_eq!(result.usage[0].native_event_id, "legacy-user-one");
    assert_eq!(result.usage[0].occurred_at, 1756684801);
}

#[test]
fn selected_legacy_session_keeps_classification_when_credits_are_appended() {
    let tmp = tempfile::tempdir().unwrap();
    let path = source(
        tmp.path(),
        "synthetic-copliot-continued",
        "jsonl",
        CONTINUED,
    );
    let result = parse(&path);
    assert!(result.complete);
    assert_eq!(result.usage.len(), 2);
    let updated = &result.usage[1];
    assert_eq!(updated.tokens.output, 8);
    assert_eq!(updated.tokens.input, 0);
    assert_eq!(updated.model, "synthetic-new");
    assert!(updated.reported_cost.is_none());
    assert_eq!(
        updated.id,
        "copliot-session:synthetic-copliot-continued:continued-new"
    );
}

#[test]
fn discovery_is_pinned_and_transcripts_and_new_sessions_do_not_add_usage() {
    let tmp = tempfile::tempdir().unwrap();
    source(tmp.path(), "synthetic-copliot-legacy", "json", LEGACY);
    let path = source(
        tmp.path(),
        "synthetic-copliot-continued",
        "jsonl",
        CONTINUED,
    );
    source(tmp.path(), "synthetic-copliot-continued", "json", LEGACY);
    source(tmp.path(), "modern", "jsonl", MODERN);
    let transcripts = tmp.path().join("transcripts");
    fs::create_dir(&transcripts).unwrap();
    fs::write(transcripts.join("synthetic-copliot-legacy.jsonl"), MODERN).unwrap();
    assert!(CopliotDeprecated.discover(tmp.path()).is_err());
    selection(
        tmp.path(),
        &["synthetic-copliot-legacy", "synthetic-copliot-continued"],
    );
    let paths = CopliotDeprecated.discover(tmp.path()).unwrap();
    assert_eq!(paths.len(), 2);
    assert!(paths.contains(&path));
}

#[test]
fn mutation_replay_uses_final_values_truncation_and_deletion() {
    let tmp = tempfile::tempdir().unwrap();
    let path = source(tmp.path(), "modern", "jsonl", MODERN);
    let (events, _, _, _) = CopliotDeprecated.read_source(&path).unwrap();
    assert!(events[0]["requests"][0].get("pendingMarker").is_none());
    assert_eq!(
        events[0]["requests"][0]["response"],
        json!([{"value":"Synthetic final response"}])
    );
    let result = parse(&path);
    assert_eq!(result.usage.len(), 3);
    assert_eq!(
        result.usage.iter().map(|r| r.tokens.output).sum::<u64>(),
        55
    );
    assert_eq!(result.usage[0].model, "synthetic-modern-a");
    // VS Code also emits truncate-only operations with no appended values.
    fs::write(
        &path,
        format!("{MODERN}{{\"kind\":2,\"k\":[\"requests\",0,\"response\"],\"i\":0}}\n"),
    )
    .unwrap();
    let (truncated, _, _, _) = CopliotDeprecated.read_source(&path).unwrap();
    assert_eq!(truncated[0]["requests"][0]["response"], json!([]));
}

#[test]
fn corrupt_sources_and_bad_counters_cannot_retract_previously_imported_records() {
    let tmp = tempfile::tempdir().unwrap();
    let path = source(
        tmp.path(),
        "synthetic-copliot-continued",
        "jsonl",
        CONTINUED,
    );
    selection(tmp.path(), &["synthetic-copliot-continued"]);
    let engine = engine(tmp.path(), tmp.path());
    assert_eq!(engine.scan_harness(ID, false).unwrap().imported, 2);
    let before = engine.lock().unwrap().local_records().unwrap();
    for broken in [
        format!("{CONTINUED}{{\"kind\":"),
        format!("{CONTINUED}{{\"kind\":1,\"k\":[\"requests\",9,\"completionTokens\"],\"v\":4}}\n"),
    ] {
        fs::write(&path, broken).unwrap();
        let report = engine.scan_harness(ID, false).unwrap();
        assert_eq!(report.deferred_files, 1);
        assert_eq!(engine.lock().unwrap().local_records().unwrap(), before);
    }
    fs::write(
        &path,
        format!("{CONTINUED}{{\"kind\":1,\"k\":[\"requests\",1,\"completionTokens\"],\"v\":-4}}\n"),
    )
    .unwrap();
    assert_eq!(engine.scan_harness(ID, false).unwrap().deferred_files, 1);
    assert_eq!(engine.lock().unwrap().local_records().unwrap(), before);
}

#[test]
fn imports_rescans_restarts_and_price_updates_only_estimate_known_tokens() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("logs");
    source(&root, "synthetic-copliot-legacy", "json", LEGACY);
    source(&root, "synthetic-copliot-continued", "jsonl", CONTINUED);
    source(&root, "modern", "jsonl", MODERN);
    selection(
        &root,
        &["synthetic-copliot-legacy", "synthetic-copliot-continued"],
    );
    let engine = engine(tmp.path(), &root);
    assert_eq!(engine.scan_harness(ID, false).unwrap().imported, 4);
    assert_eq!(
        engine.execute("models_needing_prices", json!({})).unwrap(),
        json!(["synthetic-new"])
    );
    assert!(engine
        .lock()
        .unwrap()
        .local_records()
        .unwrap()
        .iter()
        .all(|r| r.cost.amount_usd.is_none()));
    let priced = engine.execute("save_prices_detailed", json!({"entries":[{"provider":"github-copliot","modelId":"synthetic-new","displayName":"Synthetic","inputCostPerMillion":"10","outputCostPerMillion":"50","cacheReadCostPerMillion":"1","cacheCreationCostPerMillion":"1","source":"manual","sourceKey":null,"fetchedAt":1}]})).unwrap();
    assert_eq!(priced["usageChanged"], 1);
    assert_eq!(
        engine.execute("models_needing_prices", json!({})).unwrap(),
        json!([])
    );
    assert_eq!(engine.execute("reprice_usage", json!({})).unwrap(), 0);
    assert_eq!(
        engine
            .execute(
                "save_pricing_settings",
                json!({"patch":{"exchangeRates":{"CNY":"7"}}})
            )
            .unwrap()["exchangeRates"]["CNY"],
        "7"
    );
    let store = engine.lock().unwrap();
    let records = store.local_records().unwrap();
    assert_eq!(records.len(), 4);
    assert!(records.iter().all(|r| r.harness == ID
        && r.count == 1
        && (if r.tokens.total() == 0 {
            r.cost.amount_usd.is_none()
        } else {
            r.cost.amount_usd.as_deref() == Some("0.0004")
                && r.cost.method == "estimated"
                && r.cost.billing_basis.as_deref() == Some("model_reference")
        })
        && r.provider.as_deref() == Some(PROVIDER)));
    assert_eq!(
        harness_scope_core::query::summary(&records)["totalRequests"],
        4
    );
    assert_eq!(store.pending_count().unwrap(), 4);
    assert_eq!(
        harness_scope_core::query::summary(&records)["unpricedCount"],
        3
    );
    drop(store);
    assert_eq!(engine.scan_harness(ID, true).unwrap().updated, 0);
    drop(engine);
    let reopened = Engine::open(&tmp.path().join("scope.db")).unwrap();
    assert_eq!(reopened.scan_harness(ID, false).unwrap().imported, 0);
    assert_eq!(reopened.lock().unwrap().local_records().unwrap(), records);
    assert_eq!(reopened.sessions().unwrap().len(), 2);
}

#[test]
fn metadata_and_native_provider_are_preserved_and_auto_model_is_unknown() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state: Value = serde_json::from_str(LEGACY).unwrap();
    state["customTitle"] = json!("Historical work");
    state["requests"][0]["message"] = json!({"text":"Synthetic question"});
    state["requests"][0]["response"] = json!([{"value":"Synthetic answer"}]);
    state["requests"][0]["modelId"] = json!("copilot/auto");
    state["requests"][0]["providerId"] = json!("native-route");
    let path = source(
        tmp.path(),
        "synthetic-copliot-legacy",
        "json",
        &state.to_string(),
    );
    let (events, _, _, _) = CopliotDeprecated.read_source(&path).unwrap();
    let session = CopliotDeprecated.session(&path, &events).unwrap();
    assert_eq!(session.title.as_deref(), Some("Historical work"));
    assert!(!session.can_delete && session.resume_command.is_none());
    let result = parse(&path);
    assert_eq!(result.usage[0].provider.as_deref(), Some("native-route"));
    assert_eq!(result.usage[0].model, "unknown");
    let messages = CopliotDeprecated.messages(&events);
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[1]["content"], "Synthetic answer");
}

#[test]
fn missing_usage_for_an_already_priced_model_stays_unknown_without_price_lookup_loops() {
    let tmp = tempfile::tempdir().unwrap();
    let content = format!(
        "{CONTINUED}{}\n",
        json!({"kind":2,"k":["requests"],"v":[{"requestId":"missing-same-model","timestamp":1792500003000_i64,"modelId":"synthetic-new"}]})
    );
    source(tmp.path(), "synthetic-copliot-continued", "jsonl", &content);
    selection(tmp.path(), &["synthetic-copliot-continued"]);
    let engine = engine(tmp.path(), tmp.path());
    engine.scan_harness(ID, false).unwrap();
    let result = engine.execute("save_prices_detailed", json!({"entries":[{"provider":"github-copliot","modelId":"synthetic-new","displayName":"Synthetic","inputCostPerMillion":"10","outputCostPerMillion":"50","cacheReadCostPerMillion":"1","cacheCreationCostPerMillion":"1","source":"manual","sourceKey":null,"fetchedAt":1}]})).unwrap();
    assert_eq!(result["usageChanged"], 1);
    assert_eq!(
        engine.execute("models_needing_prices", json!({})).unwrap(),
        json!([])
    );
    assert_eq!(
        engine
            .execute("reprice_usage", json!({"harness":"codex"}))
            .unwrap(),
        0
    );
    let store = engine.lock().unwrap();
    let records = store.local_records().unwrap();
    assert_eq!(records.len(), 3);
    let missing = records
        .iter()
        .find(|r| r.native_event_id.as_deref() == Some("missing-same-model"))
        .unwrap();
    assert!(missing.cost.amount_usd.is_none());
    assert_eq!(
        harness_scope_core::query::summary(&records)["unpricedCount"],
        2
    );
}
