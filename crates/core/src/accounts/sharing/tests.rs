use super::*;
use crate::{model::*, sync as usage_sync};

fn engine() -> (tempfile::TempDir, Engine) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(&dir.path().join("scope.db")).unwrap();
    (dir, engine)
}
fn account(e: &Engine, shared: Option<&str>) -> Value {
    e.execute("provider_account_save",json!({"provider":"custom","name":"Same display name","sharedSubscriptionId":shared,"links":[{"harness":"codex","provider":"relay"}],"custom":{"enabled":false,"baseUrl":"","script":"","timeoutSeconds":10}})).unwrap()
}
fn bind(e: &Engine, view: &Value, shared: &str) -> Value {
    let mut save = view["account"].clone();
    save["sharedSubscriptionId"] = json!(shared);
    e.execute("provider_account_save", save).unwrap()
}
fn append(e: &Engine, id: &str, at: i64, used: i64, reset: i64) -> i64 {
    let s = e.lock().unwrap();
    let mut item = load(&s)
        .unwrap()
        .into_iter()
        .find(|i| i.account.id == id)
        .unwrap();
    item.request_id = Some(uuid::Uuid::new_v4().to_string());
    item.snapshot = Some(
        json!({"data":{"kind":"customUsage","items":[{"id":"weekly","planName":"Plan","unit":"USD","total":"100","usedPercent":used.to_string(),"resetsAt":reset,"durationMinutes":10080,"windowType":"fixed"}]}}),
    );
    s.transaction(|| {
        history::record(
            &s,
            &item,
            chrono::DateTime::from_timestamp(at, 0).unwrap(),
            "Asia/Shanghai",
        )
    })
    .unwrap();
    s.conn.last_insert_rowid()
}
fn usage(e: &Engine, id: &str, at: i64) {
    let s = e.lock().unwrap();
    let mut price = s.prices().unwrap()[0].clone();
    price.model_id = "fixture".into();
    price.provider = Some("relay".into());
    price.source = "manual".into();
    price.currency = "USD".into();
    price.pricing_rule = None;
    price.go_tariff = None;
    price.input_cost_per_million = "1".into();
    price.output_cost_per_million = "0".into();
    s.set_meta("prices", &vec![price]).unwrap();
    let row: UsageRecord = serde_json::from_value(json!({"deviceId":s.device_id().unwrap(),"id":id,"harness":"codex","provider":"relay","parserVersion":"test","sessionId":null,"nativeEventId":null,"occurredAt":at,"model":"fixture","grain":"event","count":1,"tokens":{"input":5000000,"output":0,"cacheRead":0,"cacheWrite":0,"cacheWrite1h":0,"reasoning":0},"cost":Cost::default(),"recordSource":"session","revision":1,"deleted":false,"statusCode":null,"legacyDate":null,"legacyTimezone":null})).unwrap();
    s.upsert(row, "synthetic").unwrap();
}
fn request(s: &Store, server: &Store) -> Push {
    prepare(s).unwrap();
    let mut stmt = s
        .conn
        .prepare("SELECT payload FROM subscription_observations WHERE device_id=? ORDER BY id")
        .unwrap();
    let rows = stmt
        .query_map([s.device_id().unwrap()], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    Push {
        device_id: s.device_id().unwrap(),
        server_instance_id: server.meta("server_instance_id").unwrap().unwrap(),
        subscriptions: list(s).unwrap(),
        observations: rows
            .into_iter()
            .map(|p| serde_json::from_str(&p).unwrap())
            .collect(),
    }
}
fn exchange(e: &Engine, server: &Store) {
    let s = e.lock().unwrap();
    push(server, request(&s, server)).unwrap();
    usage_sync::push(
        server,
        PushRequest {
            protocol_version: 4,
            device_id: s.device_id().unwrap(),
            device_name: "synthetic".into(),
            records: s.local_records().unwrap(),
        },
    )
    .unwrap();
    usage_sync::apply_changes(&s, &usage_sync::changes(server, 0, 500).unwrap()).unwrap();
    apply(&s, &changes(server, 0, 500).unwrap()).unwrap();
}
fn history_rows(e: &Engine, id: &str) -> Value {
    e.execute("provider_account_history", json!({"id":id}))
        .unwrap()["observations"]
        .clone()
}

#[test]
fn sequential_handoff_backfills_history_and_estimates_across_reversed_local_sequences() {
    let (_da, a) = engine();
    let (_db, b) = engine();
    let (_ds, server) = engine();
    let start = Utc::now().timestamp() - 3600;
    let reset = start + 86400;
    let av = account(&a, None);
    let aid = av["account"]["id"].as_str().unwrap();
    append(&a, aid, start, 20, reset);
    let av = bind(&a, &av, "new");
    let shared = av["account"]["sharedSubscriptionId"].as_str().unwrap();
    let bv = account(&b, None);
    let bid = bv["account"]["id"].as_str().unwrap();
    // B worked while offline. Its later observation was inserted before A's backfill.
    let end_seq = append(&b, bid, start + 1800, 30, reset);
    usage(&a, "a-request", start + 600);
    usage(&b, "b-request", start + 1200);
    exchange(&a, &server.lock().unwrap());
    exchange(&b, &server.lock().unwrap());
    assert_eq!(history_rows(&b, bid).as_array().unwrap().len(), 1); // Explicit binding required.
    let bv = bind(&b, &bv, shared);
    let rows = history_rows(&b, bid);
    assert_eq!(rows.as_array().unwrap().len(), 2);
    let start_seq = rows[1]["sequence"].as_i64().unwrap();
    assert!(start_seq > end_seq);
    let estimate = b
        .execute(
            "provider_account_estimate",
            json!({"id":bid,"firstSequence":start_seq,"lastSequence":end_seq,"windowId":"weekly"}),
        )
        .unwrap();
    assert_eq!(estimate["status"], "ready", "{estimate}");
    assert_eq!(estimate["linkedUsd"], "10");
    assert_eq!(estimate["totalEquivalentUsd"], "100");
    assert_eq!(estimate["requests"], 2);
    let page = b
        .execute("provider_account_history", json!({"id":bid,"limit":1}))
        .unwrap();
    assert_eq!(
        page["observations"][0]["observedAtMs"],
        (start + 1800) * 1000
    );
    let older = b
        .execute(
            "provider_account_history",
            json!({"id":bid,"limit":1,"beforeSequence":page["nextCursor"]}),
        )
        .unwrap();
    assert_eq!(older["observations"][0]["observedAtMs"], start * 1000);
    exchange(&b, &server.lock().unwrap());
    exchange(&a, &server.lock().unwrap());
    exchange(&b, &server.lock().unwrap());
    assert_eq!(history_rows(&a, aid).as_array().unwrap().len(), 2);
    assert_eq!(history_rows(&b, bid), rows); // Echoes and retries preserve both IDs and values.
    let ar = history_rows(&a, aid);
    let ae = a.execute("provider_account_estimate",json!({"id":aid,"firstSequence":ar[1]["sequence"],"lastSequence":ar[0]["sequence"],"windowId":"weekly"})).unwrap();
    assert_eq!(ae["totalEquivalentUsd"], estimate["totalEquivalentUsd"]);
    // Cosmetic edits and saving the binding do not invalidate measurement identity.
    let mut renamed = bv["account"].clone();
    renamed["name"] = json!("Renamed locally");
    b.execute("provider_account_save", renamed).unwrap();
    let after = b
        .execute(
            "provider_account_estimate",
            json!({"id":bid,"firstSequence":start_seq,"lastSequence":end_seq,"windowId":"weekly"}),
        )
        .unwrap();
    assert_eq!(after["status"], "ready");
    let reset_seq = append(&b, bid, start + 2400, 1, reset + 86400);
    let blocked = b.execute("provider_account_estimate",json!({"id":bid,"firstSequence":start_seq,"lastSequence":reset_seq,"windowId":"weekly"})).unwrap();
    assert_eq!(blocked["reason"], "different_window");
}

#[test]
fn interleaved_devices_keep_usage_exact_and_filter_delayed_quota_counters() {
    let (_da, a) = engine();
    let (_db, b) = engine();
    let (_ds, server) = engine();
    let start = Utc::now().timestamp() - 3600;
    let reset = start + 86400;
    let av = account(&a, Some("new"));
    let aid = av["account"]["id"].as_str().unwrap();
    exchange(&a, &server.lock().unwrap());
    exchange(&b, &server.lock().unwrap());
    let bv = account(&b, av["account"]["sharedSubscriptionId"].as_str());
    let bid = bv["account"]["id"].as_str().unwrap();
    for (i, used) in [20, 30, 25, 26, 40, 35, 40].into_iter().enumerate() {
        let (e, id) = if i % 2 == 0 { (&a, aid) } else { (&b, bid) };
        append(e, id, start + i as i64 * 180, used, reset);
    }
    usage(&a, "a-request", start + 300);
    usage(&b, "b-request", start + 600);
    for e in [&b, &a, &b, &a, &b] {
        exchange(e, &server.lock().unwrap());
    }
    for (e, id) in [(&a, aid), (&b, bid)] {
        let rows = history_rows(e, id);
        let rows = rows.as_array().unwrap();
        assert_eq!(rows.len(), 7); // No destructive correction of original history.
        let estimate = e.execute("provider_account_estimate", json!({
            "id":id,"firstSequence":rows[6]["sequence"],"lastSequence":rows[0]["sequence"],"windowId":"weekly"
        })).unwrap();
        assert_eq!(estimate["status"], "ready", "{estimate}");
        assert_eq!(estimate["ignoredObservations"], 3);
        assert_eq!(estimate["requests"], 2);
        assert_eq!(estimate["linkedUsd"], "10");
        assert_eq!(estimate["totalEquivalentUsd"], "50");
        let stale = e.execute("provider_account_estimate", json!({
            "id":id,"firstSequence":rows[6]["sequence"],"lastSequence":rows[1]["sequence"],"windowId":"weekly"
        })).unwrap();
        assert_eq!(stale["reason"], "quota_decreased");
    }
}

#[test]
fn identity_conflicts_forgery_and_nonallowlisted_data_roll_back_atomically() {
    let (_da, a) = engine();
    let (_ds, server) = engine();
    let av = account(&a, Some("new"));
    let id = av["account"]["id"].as_str().unwrap();
    append(&a, id, 1792500000, 10, 1792510000);
    let s = server.lock().unwrap();
    let local = a.lock().unwrap();
    let good = request(&local, &s);
    push(&s, request(&local, &s)).unwrap();
    push(&s, request(&local, &s)).unwrap();
    assert_eq!(changes(&s, 0, 500).unwrap().observations.len(), 1);
    let mut bad = request(&local, &s);
    bad.observations[0].data["apiKey"] = json!("never-sync");
    assert!(push(&s, bad).is_err());
    let mut bad = request(&local, &s);
    bad.device_id = "forged-device".into();
    assert!(push(&s, bad).is_err());
    let mut bad = request(&local, &s);
    bad.observations[0].data["items"][0]["usedPercent"] = json!("42");
    assert!(push(&s, bad).is_err());
    let mut bad = request(&local, &s);
    bad.server_instance_id = "old-server".into();
    assert!(push(&s, bad).is_err());
    assert_eq!(
        changes(&s, 0, 500).unwrap().observations[0].1,
        good.observations[0]
    );
    assert!(changes(&s, -1, 100).is_err());
    assert!(changes(&s, 0, 501).is_err());
    let serialized = serde_json::to_string(&good).unwrap();
    for forbidden in ["script", "baseUrl", "apiKey", "homePath", "binaryPath"] {
        assert!(!serialized.contains(&format!("\"{forbidden}\"")));
    }
}

#[test]
fn distinct_same_named_subscriptions_and_restart_keep_history_isolated() {
    let (_da, a) = engine();
    let (db, b) = engine();
    let (_ds, server) = engine();
    let av = account(&a, Some("new"));
    let bv = account(&b, Some("new"));
    let aid = av["account"]["id"].as_str().unwrap();
    let bid = bv["account"]["id"].as_str().unwrap();
    append(&a, aid, 1792500000, 10, 1792510000);
    append(&b, bid, 1792500100, 80, 1792510000);
    exchange(&a, &server.lock().unwrap());
    exchange(&b, &server.lock().unwrap());
    assert_eq!(history_rows(&b, bid).as_array().unwrap().len(), 1);
    assert!(bind_error(
        &b,
        &bv,
        av["account"]["sharedSubscriptionId"].as_str().unwrap()
    ));
    drop(b);
    let b = Engine::open(&db.path().join("scope.db")).unwrap();
    assert_eq!(
        history_rows(&b, bid)[0]["data"]["items"][0]["usedPercent"],
        "80"
    );
}
fn bind_error(e: &Engine, v: &Value, id: &str) -> bool {
    let mut save = v["account"].clone();
    save["sharedSubscriptionId"] = json!(id);
    e.execute("provider_account_save", save).is_err()
}

#[test]
fn failed_history_commit_rolls_back_outbox_and_replaying_a_refresh_keeps_the_original_sample() {
    let (_dir, e) = engine();
    let view = account(&e, Some("new"));
    let s = e.lock().unwrap();
    let mut item = load(&s).unwrap().remove(0);
    item.request_id = Some(uuid::Uuid::new_v4().to_string());
    item.snapshot = Some(
        json!({"data":{"kind":"customUsage","items":[{"id":"balance","remaining":"20","unit":"USD"}]}}),
    );
    let at = chrono::DateTime::from_timestamp(1792500000, 0).unwrap();
    s.conn.execute_batch("CREATE TRIGGER reject_sample BEFORE INSERT ON provider_account_observations BEGIN SELECT RAISE(ABORT,'disk failure'); END;").unwrap();
    assert!(s
        .transaction(|| history::record(&s, &item, at, "UTC"))
        .is_err());
    assert_eq!(pending_count(&s).unwrap(), 1); // Only the subscription descriptor.
    assert_eq!(
        s.conn
            .query_row("SELECT COUNT(*) FROM subscription_observations", [], |r| {
                r.get::<_, u64>(0)
            })
            .unwrap(),
        0
    );
    s.conn.execute_batch("DROP TRIGGER reject_sample;").unwrap();
    s.transaction(|| history::record(&s, &item, at, "UTC"))
        .unwrap();
    s.transaction(|| history::record(&s, &item, at + chrono::Duration::minutes(1), "UTC"))
        .unwrap();
    assert_eq!(pending_count(&s).unwrap(), 2);
    let value = history::query(&s, json!({"id":view["account"]["id"]})).unwrap();
    assert_eq!(value["observations"].as_array().unwrap().len(), 1);
    assert_eq!(
        value["observations"][0]["observedAtMs"],
        at.timestamp_millis()
    );
}

#[test]
fn v5_upgrade_preserves_device_and_existing_history_before_explicit_backfill() {
    let (dir, e) = engine();
    let view = account(&e, None);
    let id = view["account"]["id"].as_str().unwrap();
    append(&e, id, 1792500000, 10, 1792510000);
    let before = history_rows(&e, id);
    let device;
    {
        let s = e.lock().unwrap();
        device = s.device_id().unwrap();
        s.conn.execute_batch("DROP TABLE subscription_observations; DROP TABLE shared_subscriptions; DROP TABLE server_subscription_observations; PRAGMA user_version=5;").unwrap();
    }
    drop(e);
    let e = Engine::open(&dir.path().join("scope.db")).unwrap();
    assert_eq!(e.lock().unwrap().schema_version().unwrap(), 6);
    assert_eq!(e.lock().unwrap().device_id().unwrap(), device);
    assert_eq!(history_rows(&e, id), before);
    bind(&e, &view, "new");
    assert_eq!(history_rows(&e, id)[0]["refreshId"], before[0]["refreshId"]);
    assert_eq!(pending_count(&e.lock().unwrap()).unwrap(), 2);
}
