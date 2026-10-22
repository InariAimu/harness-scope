use harness_scope_core::{model::Source, Engine};
use serde_json::json;

#[test]
fn rename_persists_without_changing_identity_settings_or_usage() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("device.db");
    let root = dir.path().join("logs");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join("fixture.jsonl"),
        include_str!("../../../tests/fixtures/claude/session.jsonl"),
    )
    .unwrap();
    let engine = Engine::open(&path).unwrap();
    let mut config = engine.lock().unwrap().config().unwrap();
    config.sources = vec![Source {
        id: "fixture".into(),
        harness: "claude".into(),
        root: root.to_string_lossy().into(),
        enabled: true,
    }];
    config.server_token = "synthetic-private-token".into();
    config.theme = "dark".into();
    engine.lock().unwrap().set_meta("config", &config).unwrap();
    engine.scan(false).unwrap();
    let original = engine.execute("snapshot", json!({})).unwrap();
    let records = engine.lock().unwrap().local_records().unwrap();
    assert!(!records.is_empty());
    let pending = engine.lock().unwrap().pending_count().unwrap();
    let renamed = engine
        .execute("rename_device", json!({"name": "  我的 Windows 工作站  "}))
        .unwrap();
    assert_eq!(renamed, "我的 Windows 工作站");
    config.device_name = "我的 Windows 工作站".into();
    assert_eq!(
        serde_json::to_value(engine.lock().unwrap().config().unwrap()).unwrap(),
        serde_json::to_value(&config).unwrap()
    );
    drop(engine);
    let reopened = Engine::open(&path).unwrap();
    let current = reopened.execute("snapshot", json!({})).unwrap();
    assert_eq!(current["deviceId"], original["deviceId"]);
    assert_eq!(current["usageVersion"], original["usageVersion"]);
    assert_eq!(current["config"]["deviceName"], "我的 Windows 工作站");
    assert_eq!(current["config"]["serverToken"], "");
    assert_eq!(reopened.lock().unwrap().local_records().unwrap(), records);
    assert_eq!(reopened.lock().unwrap().pending_count().unwrap(), pending);
}

#[test]
fn both_name_edit_paths_reject_invalid_names_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(&dir.path().join("device.db")).unwrap();
    let original = engine.lock().unwrap().config().unwrap();
    for name in [
        "".to_owned(),
        " \n\t ".into(),
        "a\nb".into(),
        "a\0b".into(),
        "界".repeat(43),
        "x".repeat(129),
    ] {
        assert!(engine
            .execute("rename_device", json!({"name":name}))
            .is_err());
        let mut config = original.clone();
        config.device_name = name;
        assert!(engine
            .execute("save_config", json!({"config":config}))
            .is_err());
        assert_eq!(
            engine.lock().unwrap().config().unwrap().device_name,
            original.device_name
        );
    }
    assert!(engine.execute("rename_device", json!({})).is_err());
    let mut config = original;
    config.device_name = format!("  {}  ", "x".repeat(128));
    engine
        .execute("save_config", json!({"config":config}))
        .unwrap();
    assert_eq!(
        engine.lock().unwrap().config().unwrap().device_name,
        "x".repeat(128)
    );
}
