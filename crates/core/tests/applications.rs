use harness_scope_core::{
    model::{Cost, Source, UsageRecord},
    Engine,
};
use serde_json::{json, Value};

fn save(engine: &Engine, harness: &str, patch: Value) -> Value {
    engine
        .execute(
            "save_application_settings",
            json!({"harness":harness,"patch":patch}),
        )
        .unwrap()
}

fn query(engine: &Engine, kind: &str, mut args: Value) -> Value {
    args["kind"] = json!(kind);
    engine.execute("usage_query", args).unwrap()
}

#[test]
fn collection_gate_covers_normal_forced_and_targeted_scans_without_losing_directories() {
    for (harness, fixture) in [
        (
            "claude",
            include_str!("../../../tests/fixtures/claude/session.jsonl"),
        ),
        (
            "codex",
            include_str!("../../../tests/fixtures/codex/session.jsonl"),
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("sessions");
        std::fs::create_dir(&root).unwrap();
        let source = root.join("fixture.jsonl");
        std::fs::write(&source, fixture).unwrap();
        let engine = Engine::open(&dir.path().join("scope.db")).unwrap();
        let mut config = engine.lock().unwrap().config().unwrap();
        config.sources = vec![
            Source {
                id: "active".into(),
                harness: harness.into(),
                root: root.to_string_lossy().into(),
                enabled: true,
            },
            Source {
                id: "inactive".into(),
                harness: harness.into(),
                root: dir.path().join("missing").to_string_lossy().into(),
                enabled: false,
            },
        ];
        engine.lock().unwrap().set_meta("config", &config).unwrap();
        save(&engine, harness, json!({"collectionEnabled":false}));
        for force in [false, true] {
            assert_eq!(engine.scan(force).unwrap().files_scanned, 0);
            assert_eq!(
                engine.scan_harness(harness, force).unwrap().files_scanned,
                0
            );
        }
        assert!(engine.lock().unwrap().local_records().unwrap().is_empty());
        assert!(!engine.sessions().unwrap().is_empty());
        assert!(engine.sessions().unwrap().iter().all(|s| !s.can_delete));
        assert!(engine
            .execute(
                "delete_session",
                json!({"providerId":harness,"sourcePath":source,"sessionId":"fixture"})
            )
            .unwrap_err()
            .to_string()
            .contains("this application"));

        // Hidden applications still collect; the source choices are unchanged.
        save(
            &engine,
            harness,
            json!({"visible":false,"collectionEnabled":true}),
        );
        let report = engine.scan(false).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(report.imported > 0);
        let records = engine.lock().unwrap().local_records().unwrap();
        assert!(!engine.sessions().unwrap().is_empty());
        assert_eq!(query(&engine, "hasUsage", json!({})), true);
        save(
            &engine,
            harness,
            json!({"visible":true,"collectionEnabled":false}),
        );
        assert!(!engine.sessions().unwrap().is_empty());
        assert_eq!(query(&engine, "hasUsage", json!({})), true);

        // A forced scan while paused must not revoke facts even if a log changes.
        std::fs::write(&source, "").unwrap();
        assert_eq!(engine.scan(true).unwrap().files_scanned, 0);
        assert_eq!(engine.lock().unwrap().local_records().unwrap(), records);
        assert_eq!(
            serde_json::to_value(engine.lock().unwrap().config().unwrap()).unwrap(),
            json!(config)
        );
    }
}

#[test]
fn settings_persist_independently_of_config_and_reject_invalid_updates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scope.db");
    let engine = Engine::open(&path).unwrap();
    let initial = engine.execute("snapshot", json!({})).unwrap();
    assert_eq!(initial["applicationSettings"], json!({}));
    assert_eq!(
        save(&engine, "codex", json!({"collectionEnabled":false})),
        json!({"collectionEnabled":false,"visible":true})
    );
    assert_eq!(
        engine.execute("snapshot", json!({})).unwrap()["usageVersion"],
        initial["usageVersion"]
    );
    save(&engine, "codex", json!({"visible":false}));
    save(&engine, "claude", json!({"visible":false}));
    engine
        .execute("save_config", json!({"config":initial["config"]}))
        .unwrap();
    let expected = engine.execute("snapshot", json!({})).unwrap();
    for args in [
        json!({"harness":"unknown","patch":{"visible":false}}),
        json!({"harness":"codex","patch":{"visible":"false"}}),
        json!({"harness":"codex","patch":{"enabled":false}}),
        json!({"harness":"codex","patch":{}}),
        json!({"harness":"codex","patch":{"visible":null}}),
    ] {
        assert!(engine.execute("save_application_settings", args).is_err());
    }
    // Saving the same value is not another usage revision.
    save(&engine, "codex", json!({"visible":false}));
    drop(engine);
    let reopened = Engine::open(&path).unwrap();
    let actual = reopened.execute("snapshot", json!({})).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(
        actual["applicationSettings"]["codex"],
        json!({"collectionEnabled":false,"visible":false})
    );
    assert_eq!(
        actual["applicationSettings"]["claude"]["collectionEnabled"],
        true
    );
    assert_eq!(actual["harnesses"], initial["harnesses"]);
    assert_eq!(actual["usageVersion"], initial["usageVersion"]);
}

fn record(device: &str, id: &str, harness: &str) -> UsageRecord {
    serde_json::from_value(json!({
        "deviceId":device,"id":id,"harness":harness,"provider":"opencode-go","parserVersion":"fixture",
        "sessionId":"fixture","nativeEventId":id,"occurredAt":chrono::Utc::now().timestamp(),
        "model":"deepseek-flash","grain":"event","count":1,
        "tokens":{"input":100,"output":20,"cacheRead":0,"cacheWrite":0,"reasoning":0},
        "cost":Cost {amount_usd:Some("0.5".into()),method:"reported".into(),..Default::default()},"recordSource":"session","revision":1,"deleted":false
    })).unwrap()
}

#[test]
fn visibility_preserves_all_dashboard_queries_scopes_facts_sync_and_accounts() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(&dir.path().join("scope.db")).unwrap();
    let device = engine.lock().unwrap().device_id().unwrap();
    engine
        .lock()
        .unwrap()
        .upsert(record(&device, "local", "codex"), "fixture")
        .unwrap();
    let remote = record("remote", "remote", "codex");
    engine
        .lock()
        .unwrap()
        .conn
        .execute(
            "INSERT INTO remote_records VALUES(?1,?2,1,?3)",
            rusqlite::params![
                remote.device_id,
                remote.id,
                serde_json::to_string(&remote).unwrap()
            ],
        )
        .unwrap();
    let account = engine.execute("provider_account_save", json!({"provider":"opencode-go","name":"Fixture","links":[{"harness":"codex","provider":"opencode-go"}]})).unwrap();
    let account_args = json!({"id":account["account"]["id"],"range":"month"});
    let account_usage = engine
        .execute("provider_account_usage", account_args.clone())
        .unwrap();
    assert_eq!(account_usage["summary"]["totalRequests"], 2);
    let before = query(&engine, "summary", json!({"scope":"all"}));
    assert_eq!(before["totalRequests"], 2);
    let records = engine.lock().unwrap().local_records().unwrap();
    let pending = engine.lock().unwrap().pending().unwrap();
    let version = engine.lock().unwrap().usage_version().unwrap();
    let now = chrono::Utc::now().timestamp();
    let mut baseline = Vec::new();
    for scope in ["local", "all", "remote"] {
        for kind in [
            "summary",
            "todaySummary",
            "summaryByApp",
            "logs",
            "models",
            "providers",
            "trends",
            "heatmap",
            "firstDate",
            "hasUsage",
            "detail",
            "sources",
        ] {
            let args = json!({"scope":scope,"requestId":format!("{device}|local"),"startDate":now-86400,"endDate":now+1});
            baseline.push((kind, args.clone(), query(&engine, kind, args)));
        }
    }
    let harnesses = engine.execute("snapshot", json!({})).unwrap()["harnesses"].clone();
    for harness in harnesses.as_array().unwrap() {
        save(
            &engine,
            harness["id"].as_str().unwrap(),
            json!({"visible":false}),
        );
    }
    assert_eq!(engine.lock().unwrap().usage_version().unwrap(), version);
    for (kind, args, expected) in baseline {
        assert_eq!(
            query(&engine, kind, args.clone()),
            expected,
            "{kind}/{args}"
        );
    }
    for scope in ["local", "all", "remote"] {
        assert_eq!(
            query(&engine, "summary", json!({"scope":scope,"appType":"codex"}))["totalRequests"],
            if scope == "all" { 2 } else { 1 }
        );
    }
    assert_eq!(
        engine
            .execute("provider_account_usage", account_args)
            .unwrap()["summary"],
        account_usage["summary"]
    );
    assert_eq!(engine.lock().unwrap().local_records().unwrap(), records);
    assert_eq!(engine.lock().unwrap().pending().unwrap(), pending);
    assert_eq!(
        engine
            .lock()
            .unwrap()
            .conn
            .query_row("SELECT count(*) FROM remote_records", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    // Other applications still contribute.
    engine
        .lock()
        .unwrap()
        .upsert(record(&device, "other", "claude"), "fixture")
        .unwrap();
    assert_eq!(
        query(&engine, "summary", json!({"scope":"all"}))["totalRequests"],
        3
    );
    save(&engine, "codex", json!({"visible":true}));
    assert_eq!(
        query(&engine, "summary", json!({"scope":"all"}))["totalRequests"],
        3
    );
}
