use harness_scope_core::{
    harness::{
        copliot::{Copliot, ID},
        copliot_deprecated::{CopliotDeprecated, Selection, SELECTION_FILE},
        HarnessAdapter, TimelineIndex,
    },
    model::{PushRequest, Source},
    query, sync, Engine,
};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

const MODERN: &str = include_str!("../../../tests/fixtures/copliot/modern.jsonl");
const LEGACY: &str = include_str!("../../../tests/fixtures/copliot-deprecated/legacy.json");
const CONTINUED: &str = include_str!("../../../tests/fixtures/copliot-deprecated/continued.jsonl");
const SID: &str = "synthetic-copliot-modern";
fn source(root: &Path, id: &str, ext: &str, content: &str) -> PathBuf {
    let dir = root.join("workspaceStorage/project/chatSessions");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{id}.{ext}"));
    fs::write(&path, content).unwrap();
    path
}
fn selection(root: &Path, ids: &[&str]) {
    fs::write(
        root.join(SELECTION_FILE),
        serde_json::to_vec(&Selection {
            version: 1,
            session_ids: ids.iter().map(|s| (*s).into()).collect(),
        })
        .unwrap(),
    )
    .unwrap();
}
fn events(path: &Path) -> Vec<Value> {
    Copliot.read_source(path).unwrap().0
}
fn engine(tmp: &Path, root: &Path) -> Engine {
    let e = Engine::open(&tmp.join("scope.db")).unwrap();
    let s = e.lock().unwrap();
    let mut c = s.config().unwrap();
    c.sources = ["copliot-deprecated", ID]
        .into_iter()
        .map(|id| Source {
            id: id.into(),
            harness: id.into(),
            root: root.to_string_lossy().into(),
            enabled: true,
        })
        .collect();
    s.set_meta("config", &c).unwrap();
    drop(s);
    e
}

#[test]
fn modern_requests_keep_identity_resolved_model_and_only_compatible_usage() {
    let tmp = tempfile::tempdir().unwrap();
    let path = source(tmp.path(), SID, "jsonl", MODERN);
    let data = events(&path);
    let parsed = Copliot.parse(&path, &data, &TimelineIndex::new()).unwrap();
    assert!(parsed.complete && parsed.diagnostics.is_empty());
    assert_eq!(parsed.usage.len(), 3);
    assert_eq!(
        parsed.usage[0].id,
        format!("copliot-session:{SID}:modern-user-one")
    );
    assert_eq!(parsed.usage[0].model, "synthetic-modern-a");
    assert_eq!(
        parsed.usage.iter().map(|u| u.tokens.output).sum::<u64>(),
        55
    );
    assert!(parsed.usage.iter().all(|u| u.tokens.input == 0
        && u.tokens.cache_read == 0
        && u.reported_cost.is_none()
        && u.timing.is_none()
        && u.request_metadata.is_none()
        && u.provider.as_deref() == Some("github-copliot")));
    let session = Copliot.session(&path, &data).unwrap();
    assert_eq!(session.provider_id, ID);
    assert_eq!(session.session_id, SID);
    assert!(!Copliot.messages(&data).is_empty());
}

#[test]
fn shared_boundary_keeps_continued_legacy_and_discovers_future_sessions() {
    let tmp = tempfile::tempdir().unwrap();
    let modern = source(tmp.path(), SID, "jsonl", MODERN);
    source(tmp.path(), SID, "json", "{}");
    source(tmp.path(), "synthetic-copliot-legacy", "json", LEGACY);
    source(
        tmp.path(),
        "synthetic-copliot-continued",
        "jsonl",
        CONTINUED,
    );
    fs::create_dir(tmp.path().join("transcripts")).unwrap();
    fs::write(tmp.path().join("transcripts/mirror.jsonl"), MODERN).unwrap();
    assert!(Copliot.discover(tmp.path()).is_err());
    selection(
        tmp.path(),
        &["synthetic-copliot-legacy", "synthetic-copliot-continued"],
    );
    let paths = Copliot.discover(tmp.path()).unwrap();
    assert_eq!(paths, vec![modern]);
    assert!(CopliotDeprecated
        .discover(tmp.path())
        .unwrap()
        .iter()
        .all(|p| !paths.contains(p)));
    source(tmp.path(), "future-session", "json", "{}");
    assert_eq!(Copliot.discover(tmp.path()).unwrap().len(), 2);
    fs::write(
        tmp.path().join(SELECTION_FILE),
        r#"{"version":99,"sessionIds":[]}"#,
    )
    .unwrap();
    assert!(Copliot.discover(tmp.path()).is_err());
}

#[test]
fn non_copilot_agents_are_excluded_even_with_a_copilot_model_selected() {
    let tmp = tempfile::tempdir().unwrap();
    let path = source(tmp.path(), SID, "jsonl", MODERN);
    let mut data = events(&path);
    data[0]["requests"][0]["agent"]["extensionId"] = json!({"value":"other.agent"});
    data[0]["requests"][1]["isSystemInitiated"] = json!(true);
    data[0]["requests"][2]["agent"]["extensionId"] = json!("GitHub.copilot-chat");
    let parsed = Copliot.parse(&path, &data, &TimelineIndex::new()).unwrap();
    assert_eq!(parsed.usage.len(), 1);
    assert_eq!(parsed.usage[0].native_event_id, "modern-user-three");
    data[0]["requests"] = json!([]);
    assert!(Copliot.session(&path, &data).is_none());
    data[0]["inputState"]["selectedModel"]["metadata"]["extension"] =
        json!({"value":"GitHub.copilot-chat"});
    assert!(Copliot.session(&path, &data).is_some());
    assert!(Copliot
        .parse(&path, &data, &TimelineIndex::new())
        .unwrap()
        .usage
        .is_empty());
}

#[test]
fn filename_identity_cannot_bypass_the_historical_boundary() {
    let tmp = tempfile::tempdir().unwrap();
    let path = source(tmp.path(), "another-id", "jsonl", MODERN);
    assert!(Copliot.read_source(&path).is_err());
}

#[test]
fn duplicate_request_cannot_replace_the_first_observation_with_different_totals() {
    let tmp = tempfile::tempdir().unwrap();
    let path = source(tmp.path(), SID, "jsonl", MODERN);
    let mut data = events(&path);
    let mut duplicate = data[0]["requests"][0].clone();
    duplicate["modelTotals"] = json!([{"model":"unexpected-model","outputTokens":999}]);
    data[0]["requests"].as_array_mut().unwrap().push(duplicate);
    let parsed = Copliot.parse(&path, &data, &TimelineIndex::new()).unwrap();
    assert!(!parsed.complete);
    assert_eq!(parsed.usage.len(), 3);
    assert_eq!(parsed.usage[0].model, "synthetic-modern-a");
    assert_eq!(parsed.usage[0].tokens.output, 5);
}

#[test]
fn ordinary_scans_pick_up_new_sessions_and_requests_without_reclassifying_history() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("logs");
    let path = source(&root, SID, "jsonl", MODERN);
    let old = source(&root, "synthetic-copliot-continued", "jsonl", CONTINUED);
    selection(&root, &["synthetic-copliot-continued"]);
    let e = engine(tmp.path(), &root);
    e.scan_harness("copliot-deprecated", false).unwrap();
    assert_eq!(e.scan_harness(ID, false).unwrap().imported, 3);
    let mut state = events(&path).remove(0);
    let mut request = state["requests"][0].clone();
    request["requestId"] = json!("modern-user-four");
    state["requests"]
        .as_array_mut()
        .unwrap()
        .push(request.clone());
    fs::write(&path, format!("{{\"kind\":0,\"v\":{state}}}\n")).unwrap();
    state["sessionId"] = json!("future-session");
    state["requests"] = json!([request.clone()]);
    source(&root, "future-session", "json", &state.to_string());
    request["requestId"] = json!("legacy-continuation");
    fs::write(
        &old,
        format!("{CONTINUED}{{\"kind\":2,\"k\":[\"requests\"],\"v\":[{request}]}}\n"),
    )
    .unwrap();
    assert_eq!(e.scan_harness(ID, false).unwrap().imported, 2);
    assert_eq!(
        e.scan_harness("copliot-deprecated", false)
            .unwrap()
            .imported,
        1
    );
    let records = e.lock().unwrap().local_records().unwrap();
    assert_eq!(records.iter().filter(|r| r.harness == ID).count(), 5);
    assert_eq!(
        records
            .iter()
            .find(|r| r.native_event_id.as_deref() == Some("legacy-continuation"))
            .unwrap()
            .harness,
        "copliot-deprecated"
    );
    assert_eq!(e.scan_harness(ID, true).unwrap().updated, 0);
    assert_eq!(
        e.scan_harness("copliot-deprecated", true).unwrap().updated,
        0
    );
}

#[test]
fn model_totals_replace_last_call_output_without_inventing_extra_requests() {
    let tmp = tempfile::tempdir().unwrap();
    let path = source(tmp.path(), SID, "jsonl", MODERN);
    let mut data = events(&path);
    data[0]["requests"][0]["modelTotals"] = json!([
        {"model":"synthetic-totals-a","outputTokens":30,"inputTokens":1000,"cachedTokens":600},
        {"model":"synthetic-totals-b","outputTokens":10,"inputTokens":200,"cachedTokens":0}
    ]);
    let parsed = Copliot.parse(&path, &data, &TimelineIndex::new()).unwrap();
    assert_eq!(parsed.usage.len(), 3);
    assert_eq!(parsed.usage[0].model, "unknown");
    assert_eq!(parsed.usage[0].tokens.output, 40);
    assert_eq!(parsed.usage[0].tokens.input, 0);
    data[0]["requests"][0]["modelTotals"][1]["model"] = json!("synthetic-totals-a");
    assert_eq!(
        Copliot
            .parse(&path, &data, &TimelineIndex::new())
            .unwrap()
            .usage[0]
            .model,
        "synthetic-totals-a"
    );
    data[0]["requests"][0]["modelTotals"][1]["outputTokens"] = json!(-1);
    let broken = Copliot.parse(&path, &data, &TimelineIndex::new()).unwrap();
    assert!(!broken.complete);
    assert_eq!(broken.usage.len(), 2);
}

#[test]
fn imports_price_updates_and_v4_sync_preserve_counts_and_legacy_records() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("logs");
    let path = source(&root, SID, "jsonl", MODERN);
    source(&root, "synthetic-copliot-legacy", "json", LEGACY);
    selection(&root, &["synthetic-copliot-legacy"]);
    let e = engine(tmp.path(), &root);
    e.scan_harness("copliot-deprecated", false).unwrap();
    let legacy = e.lock().unwrap().local_records().unwrap();
    assert_eq!(e.scan_harness(ID, false).unwrap().imported, 3);
    assert_eq!(e.scan_harness(ID, true).unwrap().updated, 0);
    assert_eq!(
        e.execute("models_needing_prices", json!({}))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let priced = e.execute("save_prices_detailed", json!({"entries":[
        {"provider":"github-copliot","modelId":"synthetic-modern-a","displayName":"A","inputCostPerMillion":"10","outputCostPerMillion":"50","cacheReadCostPerMillion":"1","cacheCreationCostPerMillion":"1","source":"manual","sourceKey":null,"fetchedAt":1},
        {"provider":"github-copliot","modelId":"copilot/synthetic-modern-a","displayName":"A","inputCostPerMillion":"10","outputCostPerMillion":"50","cacheReadCostPerMillion":"1","cacheCreationCostPerMillion":"1","source":"manual","sourceKey":null,"fetchedAt":1}
    ]})).unwrap();
    assert_eq!(priced["usageChanged"], 2);
    assert_eq!(
        e.execute("models_needing_prices", json!({})).unwrap(),
        json!([])
    );
    assert_eq!(
        e.execute("reprice_usage", json!({"harness":ID})).unwrap(),
        0
    );
    let records = e.lock().unwrap().local_records().unwrap();
    assert!(legacy.iter().all(|r| records.contains(r)));
    let modern = records
        .iter()
        .filter(|r| r.harness == ID)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(query::summary(&modern)["totalRequests"], 3);
    assert_eq!(query::summary(&modern)["realTotalTokens"], 55);
    assert_eq!(query::summary(&modern)["totalCost"], "0.00275");
    assert_eq!(query::summary(&modern)["unpricedCount"], 1);
    assert!(modern.iter().all(|r| r.count == 1
        && (r.cost.amount_usd.is_none()
            || r.cost.billing_basis.as_deref() == Some("model_reference"))));
    let server = harness_scope_core::store::Store::open(&tmp.path().join("server.db")).unwrap();
    let device = e.lock().unwrap().device_id().unwrap();
    let push = PushRequest {
        protocol_version: 4,
        device_id: device,
        device_name: "synthetic".into(),
        records: modern,
    };
    let response = sync::push(&server, push).unwrap();
    assert_eq!(response.accepted.len(), 3);
    // A half-written operation must not revoke previously imported facts.
    fs::write(&path, format!("{MODERN}{{\"kind\":")).unwrap();
    assert_eq!(e.scan_harness(ID, false).unwrap().deferred_files, 1);
    assert_eq!(e.lock().unwrap().local_records().unwrap(), records);
    fs::write(&path, MODERN).unwrap();
    drop(e);
    let e = Engine::open(&tmp.path().join("scope.db")).unwrap();
    assert_eq!(e.scan_harness(ID, true).unwrap().updated, 0);
    assert_eq!(e.lock().unwrap().local_records().unwrap(), records);
}
