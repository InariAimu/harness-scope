use super::*;

#[test]
fn sidebar_visibility_defaults_on_and_survives_legacy_reads_edits_and_restarts() {
    let (directory, engine, _) = setup();
    let initial = create(&engine, "deepseek");
    assert_eq!(initial["account"]["showInSidebar"], true);
    let id = initial["account"]["id"].as_str().unwrap();
    {
        let store = engine.lock().unwrap();
        let mut legacy: Value = store.meta(META).unwrap().unwrap();
        legacy[0]["account"]
            .as_object_mut()
            .unwrap()
            .remove("showInSidebar");
        store.set_meta(META, &legacy).unwrap();
    }
    let legacy = engine.execute("provider_accounts", json!({})).unwrap();
    assert_eq!(legacy[0]["account"]["showInSidebar"], true);
    let mut patch = edit(&legacy[0]);
    patch["showInSidebar"] = json!(false);
    let hidden = engine.execute("provider_account_save", patch).unwrap();
    assert_eq!(hidden["account"]["showInSidebar"], false);

    // Clients that omit the new field must retain an existing opt-out.
    let mut old_client = edit(&hidden);
    old_client.as_object_mut().unwrap().remove("showInSidebar");
    old_client["name"] = json!("Renamed");
    let edited = engine.execute("provider_account_save", old_client).unwrap();
    assert_eq!(edited["account"]["showInSidebar"], false);
    assert_eq!(edited["account"]["refresh"], initial["account"]["refresh"]);
    assert!(engine.lock().unwrap().pending().unwrap().is_empty());
    drop(engine);

    let reopened = Engine::open(&directory.path().join("scope.db")).unwrap();
    let accounts = reopened.execute("provider_accounts", json!({})).unwrap();
    assert_eq!(accounts[0]["account"]["id"], id);
    assert_eq!(accounts[0]["account"]["showInSidebar"], false);
    let mut shown = edit(&accounts[0]);
    shown["showInSidebar"] = json!(true);
    assert_eq!(
        reopened.execute("provider_account_save", shown).unwrap()["account"]["showInSidebar"],
        true
    );
}
