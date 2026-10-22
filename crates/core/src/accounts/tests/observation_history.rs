use super::*;
use chrono::DateTime;

fn history(engine: &Engine, id: &str) -> Value {
    engine
        .execute("provider_account_history", json!({"id":id}))
        .unwrap()
}

fn timestamp(text: &str) -> chrono::DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Utc)
}

fn append_at(engine: &Engine, id: &str, data: Value, at: chrono::DateTime<Utc>, zone: &str) {
    let store = engine.lock().unwrap();
    let mut item = load(&store)
        .unwrap()
        .into_iter()
        .find(|i| i.account.id == id)
        .unwrap();
    item.request_id = Some(uuid::Uuid::new_v4().to_string());
    item.snapshot = Some(json!({"fetchedAt":at.timestamp(),"data":data}));
    store
        .transaction(|| super::super::history::record(&store, &item, at, zone))
        .unwrap();
}

#[test]
fn every_accepted_refresh_preserves_precise_balances_and_time_zone_without_touching_usage() {
    let (directory, engine, vault) = setup();
    let view = create(&engine, "deepseek");
    let id = view["account"]["id"].as_str().unwrap();
    let usage_version = engine.lock().unwrap().usage_version().unwrap();
    {
        let store = engine.lock().unwrap();
        let mut config = store.config().unwrap();
        config.timezone = "Asia/Shanghai".into();
        store.set_meta("config", &config).unwrap();
    }
    let initial = Utc::now().timestamp_millis();
    for _ in 0..2 {
        refresh_with(&engine, &vault, id, |provider, _| {
            let mut raw = balance("6.68249010");
            raw["apiKey"] = json!(KEY);
            parse_response(provider, raw)
        })
        .unwrap();
    }
    let rows = history(&engine, id)["observations"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(rows.len(), 2); // Equal values are samples too, never compressed away.
    assert_ne!(rows[0]["refreshId"], rows[1]["refreshId"]);
    for row in &rows {
        assert_eq!(row["data"]["balances"][0]["total"], "6.6824901");
        assert_eq!(row["timeZone"], "Asia/Shanghai");
        assert_eq!(row["utcOffsetSeconds"], 28800);
        assert_eq!(row["source"], "refresh");
        assert_eq!(row["windows"], json!([]));
        let at = row["observedAtMs"].as_i64().unwrap();
        assert!((initial..=Utc::now().timestamp_millis()).contains(&at));
        assert_eq!(
            timestamp(row["observedAtLocal"].as_str().unwrap()).timestamp_millis(),
            at
        );
        assert!(!row.to_string().contains(KEY));
    }
    let failed = refresh_with(&engine, &vault, id, |_, _| anyhow::bail!("unavailable")).unwrap();
    assert!(failed["error"].is_string());
    assert_eq!(history(&engine, id)["observations"], json!(rows));
    let store = engine.lock().unwrap();
    assert_eq!(store.usage_version().unwrap(), usage_version);
    assert!(store.pending().unwrap().is_empty());
    assert!(store.local_records().unwrap().is_empty());
    drop(store);
    drop(engine);
    let reopened = Engine::open(&directory.path().join("scope.db")).unwrap();
    assert_eq!(history(&reopened, id)["observations"], json!(rows));
}

#[test]
fn codex_window_anchors_use_duration_reset_and_dst_offsets_without_fabricating_samples() {
    let (_dir, engine, _vault) = setup();
    let view = engine
        .execute(
            "provider_account_save",
            json!({"provider":"openai-codex","name":"Codex","codex":{"plan":"pro100"}}),
        )
        .unwrap();
    let id = view["account"]["id"].as_str().unwrap();
    // The five-hour window spans the repeated 01:00 at the end of US DST.
    let at = timestamp("2026-11-01T09:30:00Z");
    let reset = timestamp("2026-11-01T12:00:00Z").timestamp();
    let data = json!({"kind":"codexQuota","identity":"f".repeat(64),"identitySource":"accountId","buckets":[
        {"id":"codex","windows":[{"id":"primary","usedPercent":12.5,"durationMinutes":300,"resetsAt":reset,"status":"ok"}]},
        {"id":"other","windows":[{"id":"secondary","usedPercent":102,"durationMinutes":10080,"resetsAt":reset,"status":"rate-limited"}]}
    ]});
    append_at(&engine, id, data, at, "America/Los_Angeles");
    let result = history(&engine, id);
    assert_eq!(result["observations"].as_array().unwrap().len(), 1);
    let row = &result["observations"][0];
    assert_eq!(row["utcOffsetSeconds"], -28800);
    assert_eq!(row["observedAtLocal"], "2026-11-01T01:30:00.000-08:00");
    let first = &row["windows"][0];
    assert_eq!(first["durationSource"], "provider");
    assert_eq!(first["remainingPercent"], "87.5");
    assert_eq!(first["startsAt"], reset - 300 * 60);
    assert_eq!(first["startsAtLocal"], "2026-11-01T00:00:00-07:00");
    assert_eq!(first["fullQuotaAnchor"]["remainingPercent"], "100");
    assert_eq!(first["fullQuotaAnchor"]["observed"], false);
    assert_eq!(first["fullQuotaAnchor"]["source"], "resetMinusDuration");
    assert_eq!(row["windows"][1]["bucketId"], "other");
    assert_eq!(row["windows"][1]["remainingPercent"], "0");
}

#[test]
fn go_fixed_windows_are_derived_but_unknown_monthly_and_stale_windows_are_not_full_samples() {
    let (_dir, engine, _vault) = setup();
    let view = create(&engine, "opencode-go");
    let id = view["account"]["id"].as_str().unwrap();
    let at = timestamp("2026-10-22T08:00:00Z");
    let data = json!({"kind":"quota","windows":[
        {"id":"rolling","usedPercent":0,"resetsAt":at.timestamp()+3600,"status":"ok"},
        {"id":"weekly","usedPercent":20,"resetsAt":at.timestamp()+86400,"status":"ok"},
        {"id":"monthly","usedPercent":3,"resetsAt":at.timestamp()+10*86400,"status":"ok"},
        {"id":"rolling","usedPercent":10,"resetsAt":at.timestamp()-1,"status":"ok"},
        {"id":"rolling","usedPercent":10,"resetsAt":at.timestamp()+3600,"status":"unknown"},
        {"id":"other","usedPercent":10,"resetsAt":i64::MAX,"status":"ok"}
    ]});
    append_at(&engine, id, data, at, "Asia/Shanghai");
    let result = history(&engine, id);
    let windows = &result["observations"][0]["windows"];
    assert_eq!(windows[0]["startsAt"], at.timestamp() - 4 * 3600);
    assert_eq!(windows[0]["durationSource"], "providerRule");
    assert_eq!(windows[1]["startsAt"], at.timestamp() - 6 * 86400);
    assert!(windows[2]["durationMinutes"].is_null());
    for i in 2..6 {
        assert!(windows[i]["fullQuotaAnchor"].is_null());
    }
    assert!(windows[4]["remainingPercent"].is_null());
}

#[test]
fn sequence_pagination_keeps_equal_timestamps_and_history_survives_account_edits_and_removal() {
    let (_dir, engine, _vault) = setup();
    let view = create(&engine, "deepseek");
    let id = view["account"]["id"].as_str().unwrap();
    let at = timestamp("2026-10-22T08:00:00.123Z");
    for value in ["1", "2", "3"] {
        append_at(
            &engine,
            id,
            parse_response("deepseek", balance(value)).unwrap(),
            at,
            "Asia/Shanghai",
        );
    }
    let first=engine.execute("provider_account_history",json!({"id":id,"limit":2,"startAtMs":at.timestamp_millis(),"endAtMs":at.timestamp_millis()+1})).unwrap();
    assert_eq!(first["observations"].as_array().unwrap().len(), 2);
    let second = engine
        .execute(
            "provider_account_history",
            json!({"id":id,"limit":2,"beforeSequence":first["nextCursor"]}),
        )
        .unwrap();
    assert_eq!(second["observations"].as_array().unwrap().len(), 1);
    assert!(second["nextCursor"].is_null());
    assert_eq!(
        second["observations"][0]["data"]["balances"][0]["total"],
        "1"
    );
    assert_eq!(
        engine
            .execute(
                "provider_account_history",
                json!({"id":id,"endAtMs":at.timestamp_millis()})
            )
            .unwrap()["observations"],
        json!([])
    );
    for args in [
        json!({"id":id,"limit":0}),
        json!({"id":id,"limit":501}),
        json!({"id":id,"beforeSequence":0}),
        json!({"id":id,"startAtMs":4,"endAtMs":3}),
    ] {
        assert!(engine.execute("provider_account_history", args).is_err());
    }
    let mut patch = edit(&view);
    patch["name"] = json!("Renamed");
    patch["apiKey"] = json!("replacement-key");
    engine.execute("provider_account_save", patch).unwrap();
    append_at(
        &engine,
        id,
        parse_response("deepseek", balance("4")).unwrap(),
        at,
        "UTC",
    );
    let rows = history(&engine, id)["observations"].clone();
    assert_eq!(rows[0]["accountName"], "Renamed");
    assert_ne!(rows[0]["accountRevision"], rows[1]["accountRevision"]);
    engine
        .execute("provider_account_delete", json!({"id":id}))
        .unwrap();
    assert_eq!(history(&engine, id)["observations"], rows);
}

#[test]
fn rejected_races_and_failed_commits_cannot_create_history_or_replace_the_latest_snapshot() {
    let (_dir, engine, vault) = setup();
    let view = create(&engine, "deepseek");
    let id = view["account"]["id"].as_str().unwrap();
    refresh_with(&engine, &vault, id, |provider, _| {
        refresh_with(&engine, &vault, id, |p, _| parse_response(p, balance("2"))).unwrap();
        parse_response(provider, balance("1"))
    })
    .unwrap();
    let before = history(&engine, id)["observations"].clone();
    assert_eq!(before.as_array().unwrap().len(), 1);
    assert_eq!(before[0]["data"]["balances"][0]["total"], "2");
    let snapshot = engine.execute("provider_accounts", json!({})).unwrap()[0]["snapshot"].clone();
    engine.lock().unwrap().conn.execute_batch("CREATE TRIGGER reject_history BEFORE INSERT ON provider_account_observations BEGIN SELECT RAISE(ABORT,'simulated disk failure'); END;").unwrap();
    assert!(refresh_with(&engine, &vault, id, |p, _| parse_response(p, balance("3"))).is_err());
    assert_eq!(history(&engine, id)["observations"], before);
    assert_eq!(
        engine.execute("provider_accounts", json!({})).unwrap()[0]["snapshot"],
        snapshot
    );
    engine
        .lock()
        .unwrap()
        .conn
        .execute_batch("DROP TRIGGER reject_history;")
        .unwrap();
    // A failure after inserting history must roll that insert back as well.
    assert!(refresh_with(&engine, &vault, id, |p, _| {
        engine.lock().unwrap().conn.execute_batch("CREATE TRIGGER reject_latest BEFORE UPDATE ON meta WHEN NEW.key='provider_accounts_v1' BEGIN SELECT RAISE(ABORT,'simulated latest-snapshot failure'); END;")?;
        parse_response(p, balance("3.5"))
    }).is_err());
    assert_eq!(history(&engine, id)["observations"], before);
    assert_eq!(
        engine.execute("provider_accounts", json!({})).unwrap()[0]["snapshot"],
        snapshot
    );
    engine
        .lock()
        .unwrap()
        .conn
        .execute_batch("DROP TRIGGER reject_latest;")
        .unwrap();
    refresh_with(&engine, &vault, id, |p, _| {
        let mut patch = edit(&view);
        patch["apiKey"] = json!("replacement-key");
        engine.execute("provider_account_save", patch).unwrap();
        parse_response(p, balance("4"))
    })
    .unwrap();
    assert_eq!(history(&engine, id)["observations"], before);
}

#[test]
fn repeated_commit_identity_is_idempotent_and_existing_v4_database_installs_history_once() {
    let (directory, engine, vault) = setup();
    let view = create(&engine, "deepseek");
    let id = view["account"]["id"].as_str().unwrap();
    refresh_with(&engine, &vault, id, |p, _| {
        parse_response(p, balance("8.12"))
    })
    .unwrap();
    {
        let store = engine.lock().unwrap();
        let item = load(&store).unwrap().remove(0);
        super::super::history::record(&store, &item, Utc::now(), "UTC").unwrap();
        assert_eq!(store.schema_version().unwrap(), 5);
    }
    assert_eq!(
        history(&engine, id)["observations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // Simulate an existing v4 installation that predates this additive local table.
    let saved = engine.execute("provider_accounts", json!({})).unwrap();
    engine
        .lock()
        .unwrap()
        .conn
        .execute_batch("DROP TABLE provider_account_observations;")
        .unwrap();
    drop(engine);
    let engine = Engine::open(&directory.path().join("scope.db")).unwrap();
    assert_eq!(
        engine.execute("provider_accounts", json!({})).unwrap(),
        saved
    );
    assert_eq!(history(&engine, id)["observations"], json!([])); // No invented old samples.
    refresh_with(&engine, &vault, id, |p, _| {
        parse_response(p, balance("8.10"))
    })
    .unwrap();
    assert_eq!(
        history(&engine, id)["observations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    drop(engine);
    let engine = Engine::open(&directory.path().join("scope.db")).unwrap();
    assert_eq!(
        history(&engine, id)["observations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
