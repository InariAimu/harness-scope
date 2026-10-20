use super::*;
#[test]
fn speed_rules_keep_subscription_and_credit_multipliers_separate_and_load_old_catalogs() {
    let mut raw = serde_json::to_value(defaults()).unwrap();
    raw.as_object_mut().unwrap().remove("subscriptionSpeed");
    let old: Catalog = serde_json::from_value(raw).unwrap();
    validate(&old).unwrap();
    assert_eq!(old.subscription_speed.fast_multiplier, "2.5");
    assert_eq!(
        old.subscription_speed.purchased_credits_fast_multiplier,
        "2"
    );
    assert!(old
        .subscription_speed
        .fast_model_ids
        .iter()
        .any(|m| m == "gpt-6.1-sol"));
    for bad in ["0", "-1", "101", "NaN"] {
        let mut c = old.clone();
        c.subscription_speed.fast_multiplier = bad.into();
        assert!(validate(&c).is_err());
    }
    let mut c = old.clone();
    c.subscription_speed
        .fast_model_ids
        .push("openai/gpt-6.1-sol".into());
    assert!(validate(&c).is_err());
    let mut c = old;
    c.subscription_speed.source_url = "javascript:test".into();
    assert!(validate(&c).is_err());
}
#[test]
fn screenshot_values_preserve_per_model_api_equivalents_and_100_million_token_units() {
    let c = defaults();
    validate(&c).unwrap();
    let model = find(&c, "openai/gpt-6.1-sol").unwrap();
    let actual = resolve(&c, model, "pro100", "weekly", "modeldial")
        .unwrap()
        .unwrap();
    assert_eq!(actual.api_equivalent_usd, "345");
    assert_eq!(actual.tokens.as_deref(), Some("1751000000"));
    assert_eq!(actual.basis, "modeldial");
    assert_eq!(
        resolve(&c, model, "plus", "weekly", "modeldial")
            .unwrap()
            .unwrap()
            .api_equivalent_usd,
        "47"
    );
    assert_eq!(
        resolve(&c, model, "plus", "rolling", "modeldial")
            .unwrap()
            .unwrap()
            .api_equivalent_usd,
        "7.8"
    );
    for (plan, usd, tokens) in [
        ("plus", "66", "43000000"),
        ("pro100", "481", "227000000"),
        ("pro200", "669", "438000000"),
        ("pro500", "1605", "1062000000"),
    ] {
        let value = resolve(
            &c,
            find(&c, "gpt-6-astra").unwrap(),
            plan,
            "weekly",
            "modeldial",
        )
        .unwrap()
        .unwrap();
        assert_eq!(value.api_equivalent_usd, usd);
        assert_eq!(value.tokens.as_deref(), Some(tokens));
    }
    assert_eq!(plan(&c, "pro100").unwrap().monthly_fee_usd, "100");
}
#[test]
fn theoretical_mode_and_missing_sample_fallback_are_explicit_and_never_use_monthly_fees() {
    let mut c = defaults();
    let model = find(&c, "gpt-6.1-sol").unwrap();
    let theory = resolve(&c, model, "pro100", "weekly", "theoretical")
        .unwrap()
        .unwrap();
    assert_eq!(theory.api_equivalent_usd, "235");
    assert_eq!(theory.basis, "theoretical");
    assert!(resolve(&c, model, "pro100", "rolling", "modeldial")
        .unwrap()
        .is_none());
    let missing = resolve(
        &c,
        find(&c, "gpt-6-sol").unwrap(),
        "pro200",
        "weekly",
        "modeldial",
    )
    .unwrap()
    .unwrap();
    assert_eq!(missing.api_equivalent_usd, "660");
    assert_eq!(missing.basis, "theoretical");
    assert_eq!(missing.tokens.as_deref(), Some("2590000000"));
    let divided = resolve(
        &c,
        find(&c, "gpt-6-astra").unwrap(),
        "plus",
        "rolling",
        "modeldial",
    )
    .unwrap()
    .unwrap();
    assert_eq!(divided.api_equivalent_usd, "11");
    assert_eq!(divided.basis, "weekly_division");
    c.theoretical_fallback = false;
    assert!(resolve(
        &c,
        find(&c, "gpt-6-sol").unwrap(),
        "pro200",
        "weekly",
        "modeldial"
    )
    .unwrap()
    .is_none());
    c.plus_weekly_to_five_hour_divisor = None;
    assert!(resolve(
        &c,
        find(&c, "gpt-6-astra").unwrap(),
        "plus",
        "rolling",
        "modeldial"
    )
    .unwrap()
    .is_none());
}
#[test]
fn invalid_or_ambiguous_local_capacity_rules_are_rejected_before_writing() {
    let c = defaults();
    for patch in [
        json!({"schemaVersion":2}),
        json!({"plans":[]}),
        json!({"defaultMode":"monthly_fee"}),
        json!({"plusWeeklyToFiveHourDivisor":"0"}),
        json!({"sourceUrl":"javascript:test"}),
    ] {
        let mut value = serde_json::to_value(&c).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        let changed: Catalog = serde_json::from_value(value).unwrap();
        assert!(validate(&changed).is_err());
    }
    let mut duplicate = c.clone();
    duplicate.models[1]
        .aliases
        .push("openai/gpt-6-astra".into());
    assert!(validate(&duplicate).is_err());
    let mut invalid = c.clone();
    invalid.models[0]
        .estimates
        .get_mut("plus")
        .unwrap()
        .weekly
        .as_mut()
        .unwrap()
        .api_equivalent_usd = "0".into();
    assert!(validate(&invalid).is_err());
    assert!(find(&c, "gpt-6.1-sol-mini").is_none());
}
#[test]
fn local_files_are_validated_drafts_with_conflict_protection_and_transaction_rollback() {
    let temp = tempfile::tempdir().unwrap();
    let engine = Engine::open(&temp.path().join("scope.db")).unwrap();
    let path = temp.path().join("scope.codex-allowances.json");
    let s = engine.lock().unwrap();
    let initial = info(&s, &path).unwrap();
    assert!(!path.exists());
    let saved=save(&s,&path,json!({"catalog":initial["catalog"],"expectedRevision":initial["revision"],"expectedFileRevision":null})).unwrap();
    let bytes = fs::read(&path).unwrap();
    let mut changed: Catalog = serde_json::from_value(saved["catalog"].clone()).unwrap();
    changed.models[0]
        .estimates
        .get_mut("plus")
        .unwrap()
        .weekly
        .as_mut()
        .unwrap()
        .api_equivalent_usd = "77".into();
    fs::write(&path, serde_json::to_vec_pretty(&changed).unwrap()).unwrap();
    assert_eq!(
        load(&s).unwrap().models[0].estimates["plus"]
            .weekly
            .as_ref()
            .unwrap()
            .api_equivalent_usd,
        "66"
    );
    assert!(save(&s,&path,json!({"catalog":changed,"expectedRevision":saved["revision"],"expectedFileRevision":saved["file"]["revision"]})).is_err());
    fs::write(&path, &bytes).unwrap();
    s.conn.execute_batch("CREATE TRIGGER reject_allowances BEFORE UPDATE ON meta WHEN NEW.key='codex_allowance_catalog_v1' BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(save(&s,&path,json!({"catalog":changed,"expectedRevision":saved["revision"],"expectedFileRevision":saved["file"]["revision"]})).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    fs::write(&path, b"invalid").unwrap();
    let invalid = info(&s, &path).unwrap();
    assert!(invalid["file"]["error"].is_string());
    assert_eq!(invalid["catalog"]["version"], saved["catalog"]["version"]);
    assert!(s.pending().unwrap().is_empty());
    assert!(s.local_records().unwrap().is_empty());
}
