use super::*;

fn order(engine: &Engine) -> Value {
    engine.execute("provider_sidebar_order", json!({})).unwrap()
}

#[test]
fn sidebar_order_persists_without_revising_accounts_prices_or_usage() {
    let (directory, engine, _) = setup();
    let a = create(&engine, "deepseek");
    let b = create(&engine, "opencode-go");
    let ids = json!([a["account"]["id"], b["account"]["id"]]);
    let reversed = json!([b["account"]["id"], a["account"]["id"]]);
    assert_eq!(order(&engine), ids);
    let before = engine.execute("provider_accounts", json!({})).unwrap();
    let account_meta: Value = engine.lock().unwrap().meta(META).unwrap().unwrap();
    let version = engine.lock().unwrap().usage_version().unwrap();
    let schema = engine.lock().unwrap().schema_version().unwrap();
    assert_eq!(
        engine
            .execute("save_provider_sidebar_order", json!({"ids":reversed}))
            .unwrap(),
        reversed
    );
    assert_eq!(order(&engine), reversed);
    assert_eq!(
        engine.execute("provider_accounts", json!({})).unwrap(),
        before
    );
    let store = engine.lock().unwrap();
    assert_eq!(store.meta::<Value>(META).unwrap().unwrap(), account_meta);
    assert_eq!(store.usage_version().unwrap(), version);
    assert_eq!(store.schema_version().unwrap(), schema);
    assert!(store.pending().unwrap().is_empty());
    drop(store);
    drop(engine);
    let reopened = Engine::open(&directory.path().join("scope.db")).unwrap();
    assert_eq!(order(&reopened), reversed);
}

#[test]
fn sidebar_order_keeps_hidden_slots_appends_new_accounts_and_discards_deleted_ids() {
    let (_directory, engine, _) = setup();
    let a = create(&engine, "deepseek");
    let b = create(&engine, "deepseek");
    let c = create(&engine, "opencode-go");
    let mut hidden = edit(&b);
    hidden["showInSidebar"] = json!(false);
    engine.execute("provider_account_save", hidden).unwrap();
    let saved = json!([c["account"]["id"], b["account"]["id"], a["account"]["id"]]);
    engine
        .execute("save_provider_sidebar_order", json!({"ids":saved}))
        .unwrap();
    assert_eq!(order(&engine), saved);
    let d = create(&engine, "deepseek");
    assert_eq!(
        order(&engine),
        json!([
            c["account"]["id"],
            b["account"]["id"],
            a["account"]["id"],
            d["account"]["id"]
        ])
    );
    // A client opened before D existed must not erase it from the order.
    engine
        .execute("save_provider_sidebar_order", json!({"ids":saved}))
        .unwrap();
    engine
        .execute("provider_account_delete", json!({"id":b["account"]["id"]}))
        .unwrap();
    assert_eq!(
        order(&engine),
        json!([c["account"]["id"], a["account"]["id"], d["account"]["id"]])
    );
}

#[test]
fn reordering_during_a_quota_fetch_preserves_the_accepted_response_and_revision() {
    let (_directory, engine, vault) = setup();
    let a = create(&engine, "deepseek");
    let b = create(&engine, "deepseek");
    let id = a["account"]["id"].as_str().unwrap();
    let reversed = json!([b["account"]["id"], a["account"]["id"]]);
    let revision = load(&engine.lock().unwrap()).unwrap()[0].revision.clone();
    let refreshed = refresh_with(&engine, &vault, id, |provider, _| {
        engine.execute("save_provider_sidebar_order", json!({"ids":reversed}))?;
        parse_response(provider, balance("18.33"))
    })
    .unwrap();
    assert_eq!(
        refreshed["snapshot"]["data"]["balances"][0]["total"],
        "18.33"
    );
    assert_eq!(load(&engine.lock().unwrap()).unwrap()[0].revision, revision);
    assert_eq!(order(&engine), reversed);
}

#[test]
fn invalid_or_failed_order_saves_do_not_change_the_last_saved_order() {
    let (_directory, engine, _) = setup();
    let a = create(&engine, "deepseek");
    let id = &a["account"]["id"];
    engine
        .execute("save_provider_sidebar_order", json!({"ids":[id]}))
        .unwrap();
    for invalid in [
        json!({"ids":[id,id]}),
        json!({"ids":["deleted-account"]}),
        json!({"ids":"bad"}),
        json!({"ids":[id],"snapshot":{}}),
    ] {
        assert!(engine
            .execute("save_provider_sidebar_order", invalid)
            .is_err());
        assert_eq!(order(&engine), json!([id]));
    }
    let b = create(&engine, "opencode-go");
    engine.lock().unwrap().conn.execute_batch("CREATE TRIGGER reject_sidebar_order BEFORE UPDATE ON meta WHEN NEW.key='provider_sidebar_order_v1' BEGIN SELECT RAISE(ABORT,'simulated order save failure'); END;").unwrap();
    assert!(engine
        .execute(
            "save_provider_sidebar_order",
            json!({"ids":[b["account"]["id"],id]})
        )
        .is_err());
    assert_eq!(order(&engine), json!([id, b["account"]["id"]]));
}
