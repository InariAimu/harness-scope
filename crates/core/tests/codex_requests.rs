use harness_scope_core::{
    harness::{codex::Codex, HarnessAdapter, TimelineIndex},
    model::{Cost, PushRequest, RequestMetadata, Source},
    store::Store,
    sync, Engine,
};
use serde_json::{json, Value};
use std::{fs, path::Path};

fn rows() -> Vec<Value> {
    let mut rows = vec![
        json!({"type":"session_meta","payload":{"id":"settings-fixture","model_provider":"openai"}}),
    ];
    let context = |effort: Value| json!({"type":"turn_context","payload":{"model":"gpt-6.1-sol","effort":effort}});
    let settings = |tier: Value| json!({"type":"event_msg","payload":{"type":"thread_settings_applied","thread_settings":{"model":"gpt-6.1-sol","service_tier":tier,"cwd":"secret-path","api_key":"secret-key"}}});
    let usage = |n| json!({"timestamp":format!("2026-10-20T01:{n:02}:00Z"),"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"output_tokens":10},"total_token_usage":{"input_tokens":n*100,"output_tokens":n*10}}}});
    rows.extend([context(json!("high")), usage(1), settings(json!("default")), context(json!("low")), usage(2),
        settings(json!("priority")), usage(3), context(json!("xhigh")), usage(4),
        settings(Value::Null), context(Value::Null), usage(5),
        json!({"type":"turn_context","payload":{"model":"gpt-6.1-sol","service_tier":"default","effort":"minimal"}}),usage(6),
        settings(json!("priority")),json!({"type":"turn_context","payload":{"model":"gpt-6-astra"}}),usage(7),
        json!({"type":"response_item","payload":{"type":"message","content":"secret-prompt"}})]);
    rows
}
fn parse(rows: &[Value]) -> Vec<harness_scope_core::model::ParsedUsage> {
    Codex
        .parse(Path::new("fixture.jsonl"), rows, &TimelineIndex::new())
        .unwrap()
        .usage
}
#[test]
fn settings_switches_bind_to_turns_without_rewriting_models_or_guessing_missing_history() {
    let parsed = parse(&rows());
    assert_eq!(parsed.len(), 7);
    let modes: Vec<_> = parsed
        .iter()
        .map(|r| r.request_metadata.as_ref().and_then(RequestMetadata::speed))
        .collect();
    assert_eq!(
        modes,
        vec![
            None,
            Some("standard"),
            Some("standard"),
            Some("fast"),
            None,
            Some("standard"),
            None
        ]
    );
    assert_eq!(
        parsed[0]
            .request_metadata
            .as_ref()
            .unwrap()
            .reasoning_effort
            .as_deref(),
        Some("high")
    );
    assert_eq!(
        parsed[2]
            .request_metadata
            .as_ref()
            .unwrap()
            .reasoning_effort
            .as_deref(),
        Some("low")
    );
    assert_eq!(
        parsed[3]
            .request_metadata
            .as_ref()
            .unwrap()
            .reasoning_effort
            .as_deref(),
        Some("xhigh")
    );
    assert!(parsed[4].request_metadata.is_none());
    assert_eq!(
        parsed[5]
            .request_metadata
            .as_ref()
            .unwrap()
            .service_tier_source
            .as_deref(),
        Some("turn_context")
    );
    for (i, r) in parsed.iter().enumerate() {
        assert_eq!(r.id, format!("codex:settings-fixture:token:{}", i + 1));
        assert_eq!(r.model, if i == 6 { "gpt-6-astra" } else { "gpt-6.1-sol" });
        assert_eq!(r.tokens.total(), 110);
    }
}
#[test]
fn projected_reader_retains_only_safe_settings_and_matches_full_reader() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fixture.jsonl");
    fs::write(
        &path,
        rows().iter().map(|r| format!("{r}\n")).collect::<String>(),
    )
    .unwrap();
    let (projected, _, errors, complete) = Codex.read_usage_source(&path).unwrap();
    assert!(errors.is_empty() && complete);
    let encoded = serde_json::to_string(&projected).unwrap();
    for secret in ["secret-key", "secret-path", "secret-prompt"] {
        assert!(!encoded.contains(secret));
    }
    let full: Vec<_> = parse(&rows())
        .into_iter()
        .map(|r| (r.id, r.model, r.tokens, r.request_metadata))
        .collect();
    let projected: Vec<_> = parse(&projected)
        .into_iter()
        .map(|r| (r.id, r.model, r.tokens, r.request_metadata))
        .collect();
    assert_eq!(full, projected);
}
#[test]
fn repeated_codex_scans_do_not_revise_unresolved_prices_only_for_a_new_quote_timestamp() {
    use harness_scope_core::model::{Price, PricingSettings};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("logs");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("session.jsonl"),
        rows().iter().map(|r| format!("{r}\n")).collect::<String>(),
    )
    .unwrap();
    let e = Engine::open(&dir.path().join("scope.db")).unwrap();
    {
        let s = e.lock().unwrap();
        let mut c = s.config().unwrap();
        c.sources = vec![Source {
            id: "fixture".into(),
            harness: "codex".into(),
            root: root.to_string_lossy().into(),
            enabled: true,
        }];
        s.set_meta("config", &c).unwrap();
        let p:Price=serde_json::from_value(json!({"modelId":"gpt-6.1-sol","displayName":"Unresolved FX fixture","provider":"openai","source":"manual","sourceKey":null,"fetchedAt":1,"currency":"CNY","inputCostPerMillion":"1","outputCostPerMillion":"1","cacheReadCostPerMillion":"0","cacheCreationCostPerMillion":"0"})).unwrap();
        s.set_meta("prices", &vec![p]).unwrap();
        s.set_meta("pricing_settings", &PricingSettings::default())
            .unwrap();
    }
    e.scan(false).unwrap();
    let before = {
        let s = e.lock().unwrap();
        for mut r in s.local_records().unwrap() {
            r.cost.priced_at = 123;
            s.upsert(r, "fixture").unwrap();
        }
        s.conn.execute("DELETE FROM outbox", []).unwrap();
        s.local_records().unwrap()
    };
    assert!(before[0].cost.amount_usd.is_none());
    assert!(before[0].cost.snapshot.is_some());
    assert_eq!(e.scan(true).unwrap().updated, 0);
    assert_eq!(e.lock().unwrap().local_records().unwrap(), before);
    assert!(e.lock().unwrap().pending().unwrap().is_empty());
}
#[test]
fn parser_upgrade_backfills_same_ids_keeps_frozen_costs_and_syncs_metadata_once() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("logs");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("session.jsonl"),
        rows().iter().map(|r| format!("{r}\n")).collect::<String>(),
    )
    .unwrap();
    let engine = Engine::open(&dir.path().join("scope.db")).unwrap();
    {
        let s = engine.lock().unwrap();
        let mut config = s.config().unwrap();
        config.sources = vec![Source {
            id: "fixture".into(),
            harness: "codex".into(),
            root: root.to_string_lossy().into(),
            enabled: true,
        }];
        s.set_meta("config", &config).unwrap();
    }
    engine.scan(false).unwrap();
    let before = {
        let s = engine.lock().unwrap();
        let mut records = s.local_records().unwrap();
        for r in &mut records {
            r.parser_version = "codex-v4-provider".into();
            r.request_metadata = None;
            r.cost = Cost::default();
            s.upsert(r.clone(), "historical").unwrap();
        }
        s.conn
            .execute(
                "UPDATE file_state SET parser_version='codex-v4-provider'",
                [],
            )
            .unwrap();
        s.conn.execute("DELETE FROM usage_read_cache", []).unwrap();
        s.conn.execute("DELETE FROM outbox", []).unwrap();
        s.local_records().unwrap()
    };
    let report = engine.scan(false).unwrap();
    assert_eq!(report.updated, 7);
    assert_eq!(report.imported, 0);
    let after = engine.lock().unwrap().local_records().unwrap();
    for old in &before {
        let new = after.iter().find(|r| r.id == old.id).unwrap();
        assert_eq!(new.tokens, old.tokens);
        assert_eq!(new.model, old.model);
        assert_eq!(new.cost, old.cost);
        assert_eq!(new.occurred_at, old.occurred_at);
        assert_eq!(new.revision, old.revision + 1);
    }
    assert_eq!(engine.scan(false).unwrap().updated, 0);
    let server = Store::open(&dir.path().join("server.db")).unwrap();
    let device = engine.lock().unwrap().device_id().unwrap();
    let pending = engine.lock().unwrap().pending().unwrap();
    let response = sync::push(
        &server,
        PushRequest {
            protocol_version: 4,
            device_id: device.clone(),
            device_name: "fixture".into(),
            records: pending.clone(),
        },
    )
    .unwrap();
    engine.lock().unwrap().ack(&response.accepted).unwrap();
    assert_eq!(engine.lock().unwrap().pending_count().unwrap(), 0);
    sync::push(
        &server,
        PushRequest {
            protocol_version: 4,
            device_id: device,
            device_name: "fixture".into(),
            records: pending,
        },
    )
    .unwrap();
    let changes = sync::changes(&server, 0, 500).unwrap();
    assert_eq!(changes.changes.len(), 7);
    let remote = Store::open(&dir.path().join("remote.db")).unwrap();
    sync::apply_changes(&remote, &changes).unwrap();
    assert_eq!(remote.records("all").unwrap(), after);
    let mut downgrade = after[3].clone();
    downgrade.request_metadata = None;
    downgrade.revision += 1;
    assert!(sync::push(
        &server,
        PushRequest {
            protocol_version: 2,
            device_id: downgrade.device_id.clone(),
            device_name: "old client".into(),
            records: vec![downgrade]
        }
    )
    .is_err());
    let log = engine
        .execute("usage_query", json!({"kind":"logs"}))
        .unwrap();
    assert!(log["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["requestMetadata"]["serviceTier"] == "priority"));
}
