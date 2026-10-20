use harness_scope_core::{model::Config, Engine};
use serde_json::json;

#[test]
fn old_configs_keep_the_plain_interface() {
    let mut old = serde_json::to_value(Config::default()).unwrap();
    old.as_object_mut().unwrap().remove("interfaceSkin");
    old.as_object_mut().unwrap().remove("skinTransparency");
    old.as_object_mut().unwrap().remove("skinMaskStrength");
    let config: Config = serde_json::from_value(old).unwrap();
    assert_eq!(config.interface_skin, "none");
    assert_eq!(config.skin_transparency, 0);
    assert_eq!(config.skin_mask_strength, 80);
}

#[test]
fn skin_survives_restart_and_can_be_disabled_without_changing_accent() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("skin.db");
    {
        let engine = Engine::open(&path).unwrap();
        let mut config = engine.lock().unwrap().config().unwrap();
        config.interface_skin = "deepseek".into();
        config.accent_color = "#5c85ee".into();
        config.skin_transparency = 35;
        config.skin_mask_strength = 65;
        engine
            .execute("save_config", json!({"config": config}))
            .unwrap();
    }
    let engine = Engine::open(&path).unwrap();
    let snapshot = engine.execute("snapshot", json!({})).unwrap();
    assert_eq!(snapshot["config"]["interfaceSkin"], "deepseek");
    assert_eq!(snapshot["config"]["skinTransparency"], 35);
    assert_eq!(snapshot["config"]["skinMaskStrength"], 65);
    let mut config = engine.lock().unwrap().config().unwrap();
    config.interface_skin = "none".into();
    engine
        .execute("save_config", json!({"config": config}))
        .unwrap();
    let config = engine.lock().unwrap().config().unwrap();
    assert_eq!(config.interface_skin, "none");
    assert_eq!(config.accent_color, "#5c85ee");
    assert_eq!(config.skin_transparency, 35);
    assert_eq!(config.skin_mask_strength, 65);
}

#[test]
fn appearance_accepts_endpoints_and_rejects_invalid_percentages_atomically() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::open(&tmp.path().join("skin.db")).unwrap();
    for (transparency, strength) in [(0, 100), (100, 0)] {
        let mut config = engine.lock().unwrap().config().unwrap();
        config.skin_transparency = transparency;
        config.skin_mask_strength = strength;
        engine
            .execute("save_config", json!({"config": config}))
            .unwrap();
    }
    let saved = serde_json::to_value(engine.lock().unwrap().config().unwrap()).unwrap();
    for key in ["skinTransparency", "skinMaskStrength"] {
        for value in [
            json!(-1),
            json!(101),
            json!(256),
            json!(12.5),
            json!("50"),
            json!(null),
        ] {
            let mut invalid = saved.clone();
            invalid[key] = value;
            assert!(engine
                .execute("save_config", json!({"config": invalid}))
                .is_err());
            let current = serde_json::to_value(engine.lock().unwrap().config().unwrap()).unwrap();
            assert_eq!(current, saved);
        }
    }
}

#[test]
fn unknown_skin_does_not_replace_saved_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = Engine::open(&tmp.path().join("skin.db")).unwrap();
    let mut config = engine.lock().unwrap().config().unwrap();
    config.interface_skin = "unknown".into();
    assert!(engine
        .execute("save_config", json!({"config": config}))
        .is_err());
    assert_eq!(
        engine.lock().unwrap().config().unwrap().interface_skin,
        "none"
    );
}
