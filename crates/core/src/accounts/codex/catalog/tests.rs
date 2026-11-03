use super::*;

#[test]
fn unchanged_previous_defaults_follow_the_new_bundle_but_edits_stay_local() {
    let old: Catalog = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/codex/allowances-2026-10-20.json"
    )))
    .unwrap();
    assert_eq!(revision(&old), PREVIOUS_DEFAULT_REVISION);
    let tmp = tempfile::tempdir().unwrap();
    let s = Store::open(&tmp.path().join("scope.db")).unwrap();
    s.set_meta(META, &old).unwrap();
    let loaded = load(&s).unwrap();
    assert_eq!(loaded.version, "modeldial-2026-11-02");
    assert!(find(&loaded, "gpt-6.1-terra").is_some());
    assert_eq!(
        revision(&s.meta::<Catalog>(META).unwrap().unwrap()),
        PREVIOUS_DEFAULT_REVISION
    );
    let mut edited = old;
    edited.models[0]
        .estimates
        .get_mut("plus")
        .unwrap()
        .weekly
        .as_mut()
        .unwrap()
        .api_equivalent_usd = "77".into();
    s.set_meta(META, &edited).unwrap();
    assert_eq!(revision(&load(&s).unwrap()), revision(&edited));
    assert!(s.pending().unwrap().is_empty());
}

#[test]
fn every_current_model_has_four_weekly_samples_and_terra_keeps_its_source_basis() {
    let c = defaults();
    assert_eq!(c.models.len(), 5);
    for model in &c.models {
        for plan in ["plus", "pro100", "pro200", "pro500"] {
            assert_eq!(
                resolve(&c, model, plan, "weekly", "modeldial")
                    .unwrap()
                    .unwrap()
                    .basis,
                "modeldial"
            );
        }
    }
    let terra = find(&c, "openai/gpt-6.1-terra").unwrap();
    for (plan, dollars, tokens) in [
        ("plus", "46.8", "475126904"),
        ("pro100", "690.0733258795349", "7005820567"),
        ("pro200", "961.8461538461538", "9415384615"),
        ("pro500", "2485.846153846154", "24276923077"),
    ] {
        let capacity = resolve(&c, terra, plan, "weekly", "modeldial")
            .unwrap()
            .unwrap();
        assert_eq!(capacity.api_equivalent_usd, dollars);
        assert_eq!(capacity.tokens.as_deref(), Some(tokens));
    }
    assert!(!c
        .subscription_speed
        .fast_model_ids
        .iter()
        .any(|id| id == "gpt-6.1-terra"));
}

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
fn current_source_values_preserve_per_model_api_equivalents_and_tokens() {
    let c = defaults();
    validate(&c).unwrap();
    let model = find(&c, "openai/gpt-6.1-sol").unwrap();
    let actual = resolve(&c, model, "pro100", "weekly", "modeldial")
        .unwrap()
        .unwrap();
    assert_eq!(actual.api_equivalent_usd, "690.0733258795349");
    assert_eq!(actual.tokens.as_deref(), Some("3502910284"));
    assert_eq!(actual.basis, "modeldial");
    assert_eq!(
        resolve(&c, model, "plus", "weekly", "modeldial")
            .unwrap()
            .unwrap()
            .api_equivalent_usd,
        "46.8"
    );
    assert_eq!(
        resolve(&c, model, "plus", "rolling", "modeldial")
            .unwrap()
            .unwrap()
            .api_equivalent_usd,
        "7.8"
    );
    for (plan, usd, tokens) in [
        ("plus", "66", "42690815"),
        ("pro100", "962", "453375300"),
        ("pro200", "1337.076923076923", "876923077"),
        ("pro500", "3210", "2123076923"),
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
    assert_eq!(theory.api_equivalent_usd, "234");
    assert_eq!(theory.basis, "theoretical");
    assert!(resolve(&c, model, "pro100", "rolling", "modeldial")
        .unwrap()
        .is_none());
    // Explicitly remove a now-available sample to exercise optional fallback.
    c.models
        .iter_mut()
        .find(|m| m.id == "gpt-6-sol")
        .unwrap()
        .estimates
        .get_mut("pro200")
        .unwrap()
        .weekly = None;
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
    assert_eq!(missing.tokens.as_deref(), Some("2592301650"));
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
