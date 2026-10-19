use harness_scope_core::{
    harness::{dsh::Dsh, HarnessAdapter, TimelineIndex},
    model::{Source, Tokens},
    Engine,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const FIXTURE: &str = include_str!("../../../tests/fixtures/dsh/session.jsonl");

fn events() -> Vec<Value> {
    FIXTURE
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
fn text(events: &[Value]) -> String {
    events.iter().map(|event| format!("{event}\n")).collect()
}
fn parse(events: &[Value]) -> harness_scope_core::harness::ParseResult {
    let path = if events[0]["version"] == 4 {
        "session.v4.jsonl"
    } else {
        "session.jsonl"
    };
    Dsh.parse(Path::new(path), events, &TimelineIndex::new())
        .unwrap()
}
fn source(dir: &Path) -> PathBuf {
    let session = dir.join("sessions/project/session-dsh-fixture");
    std::fs::create_dir_all(&session).unwrap();
    let path = session.join("session.jsonl");
    std::fs::write(&path, FIXTURE).unwrap();
    path
}
fn engine(dir: &Path, source: &Path) -> Engine {
    let engine = Engine::open(&dir.join("scope.db")).unwrap();
    let store = engine.lock().unwrap();
    let mut config = store.config().unwrap();
    config.sources = vec![Source {
        id: "dsh-fixture".into(),
        harness: "dsh".into(),
        root: source.to_string_lossy().into(),
        enabled: true,
    }];
    store.set_meta("config", &config).unwrap();
    drop(store);
    engine
}
fn renumber(events: &mut [Value]) {
    for (seq, event) in events.iter_mut().skip(1).enumerate() {
        event["seq"] = json!(seq);
    }
}
fn v4() -> Vec<Value> {
    let mut events = events();
    events[0]["version"] = json!(4);
    events[0]["isSeeded"] = json!(false);
    events.retain(|event| {
        !matches!(
            event["type"].as_str(),
            Some("assistant/chunk" | "reasoning-chunks")
        )
    });
    for event in events
        .iter_mut()
        .filter(|event| event["type"] == "assistant/message")
    {
        let start = event["time"].as_i64().unwrap() - 1500;
        event["data"]["stream"] = json!([{"type":"text-chunks","time0":start,"index":0,"dt":[100,100],"texts":["","hello"," there"]}]);
    }
    renumber(&mut events);
    events
}

#[test]
fn app_rc2_native_v4_shape_counts_settlement_once_and_hides_runtime_context() {
    let rows: Vec<Value> = include_str!("../../../tests/fixtures/dsh/session.v4.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let result = parse(&rows);
    assert!(result.complete && result.diagnostics.is_empty());
    assert_eq!(result.usage.len(), 1); // embedded usage is the same settlement, not another call.
    assert_eq!(result.usage[0].tokens.total(), 130);
    assert_eq!(
        result.usage[0].timing.as_ref().unwrap().first_token_ms,
        Some(500)
    );
    assert_eq!(result.usage[0].timing.as_ref().unwrap().duration_ms, 2000);
    let messages = Dsh.messages(&rows);
    assert_eq!(messages.len(), 3);
    assert_eq!(
        messages
            .iter()
            .filter(|m| m["role"] == "user" && m["injected"] != true)
            .count(),
        1
    );
    assert_eq!(messages[2]["blocks"][1]["text"], "Synthetic App reply");
}

#[test]
fn multiframe_zstd_disjoint_usage_timing_and_transcript() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("session.jsonl.zstd");
    let (h, body) = FIXTURE.split_once('\n').unwrap();
    let mut bytes = zstd::encode_all(format!("{h}\n").as_bytes(), 1).unwrap();
    bytes.extend(zstd::encode_all(body.as_bytes(), 1).unwrap());
    std::fs::write(&path, &bytes).unwrap();
    let (rows, _, diagnostics, complete) = Dsh.read_source(&path).unwrap();
    assert!(complete && diagnostics.is_empty());
    let result = parse(&rows);
    assert!(result.complete && result.diagnostics.is_empty());
    assert_eq!(result.usage.len(), 2);
    let first = &result.usage[0];
    assert_eq!(first.id, "dsh:session-dsh-fixture:message-one");
    assert_eq!(
        first.tokens,
        Tokens {
            input: 100,
            output: 30,
            cache_read: 40,
            cache_write: 20,
            reasoning: 8,
            cache_write1h: 0
        }
    );
    assert_eq!(first.tokens.total(), 190); // reasoning is already inside output.
    assert!(first.reported_cost.is_none());
    let timing = first.timing.as_ref().unwrap();
    assert_eq!(timing.duration_ms, 2000); // tool execution is outside the model window.
    assert_eq!(timing.first_token_ms, Some(500)); // first packed item is empty.
    let session = Dsh.session(&path, &rows).unwrap();
    assert_eq!(session.title.as_deref(), Some("DSH fixture"));
    assert_eq!(session.project_dir.as_deref(), Some("/synthetic/dsh"));
    assert!(!Dsh.descriptor().delete);
    assert!(Dsh.delete_source(&path).is_err());
    let messages = Dsh.messages(&rows);
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[1]["blocks"][0]["type"], "thinking");
    assert_eq!(messages[1]["blocks"][2]["type"], "tool_call");
    assert_eq!(messages[2]["blocks"][0]["status"], "success");
    assert_eq!(messages[2]["blocks"][0]["callId"], "call-one");
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let mut context = rows[1].clone();
    context["data"]["source"] = json!({"kind":"runtime-context","form":"snapshot"});
    assert_eq!(Dsh.messages(&[context])[0]["injected"], true);
    assert_eq!(messages[0]["injected"], false);
}

#[test]
fn restarts_append_corrections_and_partial_writes_preserve_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let path = source(tmp.path());
    let e = engine(tmp.path(), &tmp.path().join("sessions"));
    assert_eq!(e.scan(false).unwrap().imported, 2);
    assert_eq!(e.scan(false).unwrap().imported, 0);
    let original = e.lock().unwrap().local_records().unwrap();
    std::fs::write(&path, format!("{FIXTURE}{{\"type\":")).unwrap();
    let partial = e.scan(false).unwrap();
    assert_eq!(partial.deferred_files, 1);
    assert_eq!(e.lock().unwrap().local_records().unwrap(), original);
    let mut rows = events();
    let call = rows
        .iter_mut()
        .find(|e| e["type"] == "assistant/message")
        .unwrap();
    call["data"]["usage"]["outputTokens"] = json!(35);
    std::fs::write(&path, text(&rows)).unwrap();
    assert_eq!(e.scan(false).unwrap().updated, 1);
    let mut extra = rows
        .iter()
        .find(|e| e["type"] == "assistant/message")
        .unwrap()
        .clone();
    extra["seq"] = json!(16);
    extra["data"]["message"]["id"] = json!("message-three");
    rows.push(extra);
    std::fs::write(&path, text(&rows)).unwrap();
    assert_eq!(e.scan(false).unwrap().imported, 1);
    assert_eq!(e.lock().unwrap().local_records().unwrap().len(), 3);
    assert_eq!(e.scan(true).unwrap().updated, 0);
    let pending = serde_json::to_string(&e.lock().unwrap().pending().unwrap()).unwrap();
    assert!(!pending.contains("synthetic/dsh") && !pending.contains("synthetic answer"));
    drop(e);
    let restarted = Engine::open(&tmp.path().join("scope.db")).unwrap();
    assert_eq!(restarted.scan(false).unwrap().imported, 0);
    assert_eq!(restarted.lock().unwrap().local_records().unwrap().len(), 3);
}

#[test]
fn migrated_v4_is_preferred_without_recounting_v0_messages() {
    let tmp = tempfile::tempdir().unwrap();
    let path = source(tmp.path());
    let e = engine(tmp.path(), tmp.path()); // A DSH home is accepted as well as sessions/.
    assert_eq!(e.scan(false).unwrap().imported, 2);
    let upgraded = path.with_file_name("session.v4.jsonl");
    let rows = v4();
    std::fs::write(&upgraded, text(&rows)).unwrap();
    assert_eq!(Dsh.discover(tmp.path()).unwrap(), vec![upgraded.clone()]);
    let result = parse(&rows);
    assert!(result.complete);
    assert_eq!(
        result.usage[0].timing.as_ref().unwrap().first_token_ms,
        Some(600)
    );
    assert_eq!(e.scan(false).unwrap().imported, 0);
    let records = e.lock().unwrap().local_records().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records.iter().map(|r| r.tokens.total()).sum::<u64>(), 330);
    assert_eq!(e.sessions().unwrap().len(), 1);
    std::fs::remove_file(path).unwrap();
    assert_eq!(e.scan(true).unwrap().updated, 0);
    assert_eq!(
        Path::new(e.sessions().unwrap()[0].source_path.as_deref().unwrap()),
        upgraded
    );
}

#[test]
fn inherited_prefix_replacement_nodes_and_stream_usage_are_not_new_calls() {
    let mut rows = events();
    rows[0]["seedLength"] = json!(11);
    rows[0]["parentSession"] = json!("session-parent");
    assert_eq!(parse(&rows).usage.len(), 1);
    rows[0].as_object_mut().unwrap().remove("seedLength");
    assert!(Dsh
        .parse(Path::new("session.jsonl"), &rows, &TimelineIndex::new())
        .is_err());
    rows[0]["origin"] = json!("subagent");
    assert_eq!(parse(&rows).usage.len(), 2); // A fresh subagent has no inherited prefix.
    let mut rows = v4();
    rows[0]["isSeeded"] = json!(true);
    let second = rows
        .iter()
        .position(|e| e["type"] == "step/start" && e["data"]["step"] == 2)
        .unwrap();
    rows.insert(
        second,
        json!({"type":"session/end-seed","time":1792112405102_i64,"data":{}}),
    );
    renumber(&mut rows);
    assert!(Dsh
        .parse(Path::new("session.v4.jsonl"), &rows, &TimelineIndex::new())
        .is_err());
    rows[second]["data"]["inherited"] = json!(true);
    // An older fork marker and a later ordinary resume must not move the final cut.
    rows.insert(
        1,
        json!({"type":"session/end-seed","time":1792112400000_i64,"data":{"inherited":true}}),
    );
    rows.push(json!({"type":"session/end-seed","time":1792112407000_i64,"data":{}}));
    renumber(&mut rows);
    assert_eq!(parse(&rows).usage.len(), 1);
    rows[0]["seedLength"] = json!(0);
    assert!(Dsh
        .parse(Path::new("session.v4.jsonl"), &rows, &TimelineIndex::new())
        .is_err());
    rows[0].as_object_mut().unwrap().remove("seedLength");
    rows[0]["isSeeded"] = json!(false);
    assert!(Dsh
        .parse(Path::new("session.v4.jsonl"), &rows, &TimelineIndex::new())
        .is_err());
    let mut rows = v4();
    let first = rows
        .iter()
        .find(|e| e["type"] == "assistant/message")
        .unwrap()
        .clone();
    let mut replay = first.clone();
    replay["data"]["message"]["id"] = json!("replayed-copy");
    replay["surfaceOp"] = json!({"op":"replace","start":0,"end":1});
    rows.push(replay);
    rows.push(first.clone()); // same native message ID stays one fact.
    let mut host = first;
    host["data"]["message"]["id"] = json!("usage-host");
    host["data"]["message"]["content"] = json!([]);
    host.as_object_mut().unwrap().remove("surfaceOp");
    rows.push(host);
    renumber(&mut rows);
    assert_eq!(parse(&rows).usage.len(), 3);
    let mut result_rows = v4();
    let result = result_rows
        .iter_mut()
        .find(|e| e["type"] == "tool/result")
        .unwrap();
    result["data"]["message"] = json!({"id":"tool-result-v4","role":"tool","source":{"kind":"tool","callId":"call-one"},"content":[{"type":"text","text":"synthetic failure"}],"isError":true});
    assert_eq!(
        Dsh.messages(&result_rows)[2]["blocks"][0]["status"],
        "error"
    );
}

#[test]
fn app_v4_embedded_retry_stream_sets_first_token_without_counting_attempts() {
    let mut rows = v4();
    let message_index = rows
        .iter()
        .position(|e| e["type"] == "assistant/message")
        .unwrap();
    let mut attempt = rows[message_index].clone();
    attempt["type"] = json!("assistant/attempt");
    attempt["data"]["stream"] = json!([
        {"type":"chunk","time":1792112400200_i64,"chunk":{"type":"block-start","blockType":"text","index":0}},
        {"type":"text-chunks","time0":1792112400300_i64,"dt":[100],"texts":["","partial"],"index":0}
    ]);
    rows.insert(message_index, attempt);
    renumber(&mut rows);
    let parsed = parse(&rows);
    assert!(parsed.complete);
    assert_eq!(parsed.usage.len(), 2);
    assert_eq!(
        parsed.usage[0].timing.as_ref().unwrap().first_token_ms,
        Some(300)
    );
}

#[test]
fn canonical_generations_and_header_versions_are_checked_without_fallback() {
    let tmp = tempfile::tempdir().unwrap();
    let old = source(tmp.path());
    for name in [
        "session.v0.jsonl",
        "session.v04.jsonl",
        "session.v+4.jsonl",
        "session.V4.jsonl",
        "session.v4.jsonl.tmp",
    ] {
        std::fs::write(old.with_file_name(name), text(&v4())).unwrap();
    }
    assert_eq!(Dsh.discover(tmp.path()).unwrap(), vec![old.clone()]);
    let path = old.with_file_name("session.v5.jsonl");
    std::fs::write(&path, text(&v4())).unwrap();
    assert_eq!(Dsh.discover(tmp.path()).unwrap(), vec![path.clone()]);
    assert!(Dsh.parse(&path, &v4(), &TimelineIndex::new()).is_err());
    assert!(Dsh.parse(&old, &v4(), &TimelineIndex::new()).is_err());
    std::fs::remove_file(path).unwrap();
    std::fs::write(
        old.with_file_name("session.jsonl.zstd"),
        zstd::encode_all(FIXTURE.as_bytes(), 1).unwrap(),
    )
    .unwrap();
    assert!(Dsh.discover(tmp.path()).is_err());
}

#[test]
fn invalid_or_missing_usage_gaps_and_unknown_formats_are_not_zero() {
    for (field, value) in [
        ("inputTokens", json!(-1)),
        ("outputTokens", Value::Null),
        ("cacheReadTokens", json!("40")),
        ("reasoningTokens", json!(999)),
    ] {
        let mut rows = events();
        rows.iter_mut()
            .find(|e| e["type"] == "assistant/message")
            .unwrap()["data"]["usage"][field] = value;
        let result = parse(&rows);
        assert!(!result.complete && !result.diagnostics.is_empty());
        assert_eq!(result.usage.len(), 1);
    }
    let mut rows = events();
    rows.iter_mut()
        .find(|e| e["type"] == "assistant/message")
        .unwrap()["data"]
        .as_object_mut()
        .unwrap()
        .remove("usage");
    assert!(!parse(&rows).complete);
    let mut rows = events();
    rows[2]["seq"] = json!(999);
    assert!(!parse(&rows).complete);
    assert!(parse(&rows).usage.is_empty());
    let mut rows = events();
    rows.iter_mut()
        .find(|e| e["type"] == "reasoning-chunks")
        .unwrap()["data"]["dt"] = json!([]);
    assert!(!parse(&rows).complete);
    rows[0]["version"] = json!(999);
    assert!(Dsh
        .parse(Path::new("session.jsonl"), &rows, &TimelineIndex::new())
        .is_err());
}

#[test]
fn incomplete_zstd_is_retried_without_revoking_completed_usage() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("session.jsonl.zstd");
    let bytes = zstd::encode_all(FIXTURE.as_bytes(), 1).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let e = engine(tmp.path(), &path);
    assert_eq!(e.scan(false).unwrap().imported, 2);
    let before = e.lock().unwrap().local_records().unwrap();
    let mut torn = bytes.clone();
    let mut frame = zstd::encode_all(
        b"{\"type\":\"turn/start\",\"seq\":16,\"time\":1792112407000,\"data\":{\"turn\":2}}\n"
            .as_slice(),
        1,
    )
    .unwrap();
    let last_byte = frame.pop().unwrap();
    torn.extend(&frame);
    std::fs::write(&path, &torn).unwrap();
    assert!(!e.scan(false).unwrap().errors.is_empty());
    assert_eq!(e.lock().unwrap().local_records().unwrap(), before);
    torn.push(last_byte);
    std::fs::write(&path, &torn).unwrap();
    assert!(e.scan(false).unwrap().errors.is_empty());
    assert_eq!(e.lock().unwrap().local_records().unwrap(), before);
}
