use super::*;

fn setup() -> (tempfile::TempDir, Engine) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(&dir.path().join("icons.db")).unwrap();
    (dir, engine)
}

fn svg_url(content: &str) -> String {
    format!("data:image/svg+xml;base64,{}", STANDARD.encode(content))
}

fn svg() -> String {
    svg_url(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="100 0 300 300"><svg width="500" height="300" viewBox="0 0 500 300"><defs><linearGradient id="paint"><stop offset="0" stop-color="#abc"/></linearGradient></defs><rect width="500" height="300" fill="url(#paint)"/></svg></svg>"##,
    )
}

fn png_url(width: u32, height: u32) -> String {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&vec![128; (width * height * 4) as usize])
            .unwrap();
    }
    format!("data:image/png;base64,{}", STANDARD.encode(bytes))
}

fn import(engine: &Engine, target: Value, data: &str) -> Result<Value> {
    engine.execute(
        "import_entity_icon",
        json!({"target":target,"name":"Mark.svg","dataUrl":data}),
    )
}

#[test]
fn all_bundled_presets_can_be_saved_for_harnesses_and_accounts() {
    let (dir, engine) = setup();
    let account = engine
        .execute(
            "provider_account_save",
            json!({"provider":"deepseek","name":"Account"}),
        )
        .unwrap();
    let id = account["account"]["id"].as_str().unwrap().to_owned();
    for preset in presets() {
        let icon = format!("builtin:{}", preset.id);
        for (kind, target_id, field) in [
            ("harness", "claude", "harnesses"),
            ("account", id.as_str(), "accounts"),
        ] {
            let library = engine
                .execute(
                    "set_entity_icon",
                    json!({"target":{"kind":kind,"id":target_id},"icon":icon}),
                )
                .unwrap();
            assert_eq!(library[field][target_id], icon);
            assert!(library["assets"].as_object().unwrap().is_empty());
        }
    }
    let before = engine.execute("icon_library", json!({})).unwrap();
    drop(engine);
    let reopened = Engine::open(&dir.path().join("icons.db")).unwrap();
    assert_eq!(reopened.execute("icon_library", json!({})).unwrap(), before);
}

#[test]
fn icons_persist_deduplicate_and_do_not_revise_accounts_or_usage() {
    let (dir, engine) = setup();
    let account = engine
        .execute(
            "provider_account_save",
            json!({"provider":"deepseek","name":"Account"}),
        )
        .unwrap();
    let id = account["account"]["id"].as_str().unwrap();
    let before: Value = engine
        .lock()
        .unwrap()
        .meta("provider_accounts_v1")
        .unwrap()
        .unwrap();
    let version = engine.lock().unwrap().usage_version().unwrap();
    let initial = engine.execute("icon_library", json!({})).unwrap();
    assert_eq!(initial["revision"], 0);
    let library = import(&engine, json!({"kind":"harness","id":"claude"}), &svg()).unwrap();
    let reference = library["harnesses"]["claude"].clone();
    let library = import(&engine, json!({"kind":"account","id":id}), &svg()).unwrap();
    assert_eq!(library["assets"].as_object().unwrap().len(), 1);
    assert_eq!(library["accounts"][id], reference);
    assert_eq!(
        engine
            .lock()
            .unwrap()
            .meta::<Value>("provider_accounts_v1")
            .unwrap()
            .unwrap(),
        before
    );
    assert_eq!(engine.lock().unwrap().usage_version().unwrap(), version);
    assert!(engine.lock().unwrap().pending().unwrap().is_empty());
    drop(engine);
    let reopened = Engine::open(&dir.path().join("icons.db")).unwrap();
    assert_eq!(
        reopened.execute("icon_library", json!({})).unwrap(),
        library
    );
}

#[test]
fn resetting_and_deleting_accounts_release_references_but_keep_reusable_assets() {
    let (_dir, engine) = setup();
    let account = engine
        .execute(
            "provider_account_save",
            json!({"provider":"deepseek","name":"Account"}),
        )
        .unwrap();
    let account_id = account["account"]["id"].as_str().unwrap();
    let library = import(&engine, json!({"kind":"account","id":account_id}), &svg()).unwrap();
    let id = library["assets"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    engine
        .execute(
            "set_entity_icon",
            json!({"target":{"kind":"harness","id":"claude"},"icon":format!("asset:{id}")}),
        )
        .unwrap();
    assert!(engine
        .execute("delete_icon_asset", json!({"id":id}))
        .is_err());
    let reset = engine
        .execute(
            "set_entity_icon",
            json!({"target":{"kind":"harness","id":"claude"},"icon":null}),
        )
        .unwrap();
    assert!(reset["harnesses"]["claude"].is_null());
    assert_eq!(reset["assets"].as_object().unwrap().len(), 1);
    assert!(engine
        .execute("delete_icon_asset", json!({"id":id}))
        .is_err());
    engine
        .execute("provider_account_delete", json!({"id":account_id}))
        .unwrap();
    let library = engine.execute("icon_library", json!({})).unwrap();
    assert!(library["accounts"].as_object().unwrap().is_empty());
    assert_eq!(library["assets"].as_object().unwrap().len(), 1);
    let deleted = engine
        .execute("delete_icon_asset", json!({"id":id}))
        .unwrap();
    assert!(deleted["assets"].as_object().unwrap().is_empty());
    assert!(import(&engine, json!({"kind":"account","id":account_id}), &svg()).is_err());
}

#[test]
fn invalid_or_failed_writes_preserve_the_previous_library() {
    let (_dir, engine) = setup();
    let before = engine
        .execute(
            "set_entity_icon",
            json!({"target":{"kind":"harness","id":"claude"},"icon":"builtin:deepseek"}),
        )
        .unwrap();
    for invalid in [
        json!({"target":{"kind":"harness","id":"missing"},"icon":"builtin:claude"}),
        json!({"target":{"kind":"unknown","id":"claude"},"icon":"builtin:claude"}),
        json!({"target":{"kind":"account","id":"missing"},"icon":"builtin:claude"}),
        json!({"target":{"kind":"harness","id":"claude"},"icon":"builtin:missing"}),
        json!({"target":{"kind":"harness","id":"claude"},"icon":"asset:missing"}),
    ] {
        assert!(engine.execute("set_entity_icon", invalid).is_err());
        assert_eq!(engine.execute("icon_library", json!({})).unwrap(), before);
    }
    engine.lock().unwrap().conn.execute_batch("CREATE TRIGGER fail_icon_save BEFORE UPDATE ON meta WHEN NEW.key='icon_library_v1' BEGIN SELECT RAISE(ABORT,'simulated save failure'); END;").unwrap();
    assert!(import(&engine, json!({"kind":"harness","id":"claude"}), &svg()).is_err());
    assert_eq!(engine.execute("icon_library", json!({})).unwrap(), before);
}

#[test]
fn image_validation_enforces_decoded_dimensions_and_static_svg() {
    assert_eq!(validate_asset(&png_url(128, 128)).unwrap().1, 128);
    assert!(validate_asset(&png_url(129, 128)).is_err());
    assert!(validate_asset("data:image/png;base64,aGVsbG8=").is_err());
    assert!(validate_asset("https://example.com/icon.svg").is_err());
    assert!(validate_asset(&format!(
        "data:image/png;base64,{}",
        "A".repeat(MAX_BYTES * 2)
    ))
    .is_err());
    assert_eq!(validate_asset(&svg()).unwrap().2, 128);
    for bad in [
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="129" height="128"/>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="0" height="128"/>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" onload="alert(1)"/>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128"><script>alert(1)</script></svg>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128"><image href="https://example.com/a.png"/></svg>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128"><foreignObject/></svg>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128"><path fill="url(https://example.com/x)"/></svg>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128"><path style="fill:red"/></svg>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128"><path fill="u\72l(https://example.com)"/></svg>"#,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" xml:base="https://example.com"><path fill="url(#a)"/></svg>"##,
        r#"<!DOCTYPE svg [<!ENTITY x "boom">]><svg xmlns="http://www.w3.org/2000/svg" width="128" height="128">&x;</svg>"#,
    ] {
        assert!(validate_asset(&svg_url(bad)).is_err(), "accepted {bad}");
    }
}
