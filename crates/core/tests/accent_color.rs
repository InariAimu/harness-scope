use harness_scope_core::{model::Config, Engine};
use serde_json::json;

#[test]
fn older_configs_default_to_the_original_accent() {
    let mut old = serde_json::to_value(Config::default()).unwrap();
    old.as_object_mut().unwrap().remove("accentColor");
    let config: Config = serde_json::from_value(old).unwrap();
    assert_eq!(config.accent_color, "#f97316");
}

#[test]
fn accent_is_normalized_and_persisted_across_restarts() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("accent.db");
    {
        let engine = Engine::open(&path).unwrap();
        let mut config = engine.lock().unwrap().config().unwrap();
        config.accent_color = "#2563EB".into();
        engine
            .execute("save_config", json!({"config": config}))
            .unwrap();
        assert_eq!(
            engine.execute("snapshot", json!({})).unwrap()["config"]["accentColor"],
            "#2563eb"
        );
    }
    let engine = Engine::open(&path).unwrap();
    assert_eq!(
        engine.lock().unwrap().config().unwrap().accent_color,
        "#2563eb"
    );
}

#[test]
fn invalid_accents_do_not_replace_saved_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::open(&tmp.path().join("accent.db")).unwrap();
    for value in ["", "red", "#abc", "#12345g", "#12345678", "var(--danger)"] {
        let mut config = engine.lock().unwrap().config().unwrap();
        config.accent_color = value.into();
        assert!(engine
            .execute("save_config", json!({"config": config}))
            .is_err());
        assert_eq!(
            engine.lock().unwrap().config().unwrap().accent_color,
            "#f97316"
        );
    }
}
