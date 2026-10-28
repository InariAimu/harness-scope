use harness_scope_core::Engine;
use serde_json::{json, Value};

fn patch(engine: &Engine, patch: Value) -> Value {
    engine
        .execute("patch_config", json!({"patch":patch}))
        .unwrap()
}

#[test]
fn autosave_preserves_credentials_sources_and_concurrent_renames() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.db");
    let engine = Engine::open(&path).unwrap();
    patch(&engine, json!({"serverToken":"test-secret"}));
    let sources = engine.execute("snapshot", json!({})).unwrap()["config"]["sources"].clone();
    engine
        .execute("rename_device", json!({"name":"Other edit"}))
        .unwrap();
    let result = patch(&engine, json!({"theme":"dark", "language":"en"}));
    assert_eq!(result["deviceName"], "Other edit");
    assert_eq!(result["serverToken"], "");
    assert_eq!(result["sources"], sources);
    assert_eq!(
        engine.lock().unwrap().config().unwrap().server_token,
        "test-secret"
    );
    drop(engine);
    let engine = Engine::open(&path).unwrap();
    let result = engine.execute("snapshot", json!({})).unwrap();
    assert_eq!(result["config"]["theme"], "dark");
    assert_eq!(result["config"]["language"], "en");
    assert_eq!(result["tokenConfigured"], true);
}

#[test]
fn invalid_or_unknown_patches_leave_all_preferences_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(&dir.path().join("settings.db")).unwrap();
    let before = patch(&engine, json!({"theme":"light"}));
    for value in [
        json!({}),
        json!({"sources":[]}),
        json!({"launchOnStartup":true}),
        json!({"language":"unsupported"}),
        json!({"theme":"invalid"}),
        json!({"theme":"dark","timezone":"invalid"}),
        json!({"scanIntervalSeconds":0}),
        json!({"scanIntervalSeconds":1.5}),
        json!({"deviceName":"  "}),
        json!({"accentColor":"#xyzxyz"}),
        json!({"skinTransparency":101}),
        json!({"theme":null}),
    ] {
        assert!(
            engine
                .execute("patch_config", json!({"patch":value}))
                .is_err(),
            "{value}"
        );
        assert_eq!(
            engine.execute("snapshot", json!({})).unwrap()["config"],
            before
        );
    }
}

#[test]
fn sync_form_patch_does_not_revert_general_autosaves_and_blank_token_is_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(&dir.path().join("settings.db")).unwrap();
    patch(&engine, json!({"serverToken":"test-secret"}));
    patch(
        &engine,
        json!({"accentColor":"#ABCDEF", "deviceName":"  Trimmed  ", "scanIntervalSeconds":5}),
    );
    let result = patch(
        &engine,
        json!({"serverUrl":"https://example.invalid", "serverToken":""}),
    );
    assert_eq!(result["accentColor"], "#abcdef");
    assert_eq!(result["deviceName"], "Trimmed");
    assert_eq!(result["scanIntervalSeconds"], 5);
    assert_eq!(
        engine.lock().unwrap().config().unwrap().server_token,
        "test-secret"
    );
    assert_eq!(result["serverToken"], "");
}
