use super::*;
use crate::model::{Cost, Tokens, UsageRecord};

const KEY: &str = "test-local-provider-secret-do-not-echo";
fn setup() -> (tempfile::TempDir, Engine, Vault) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("scope.db");
    let engine = Engine::open(&path).unwrap();
    (directory, engine, Vault::new(&path))
}
fn create(engine: &Engine, provider: &str) -> Value {
    engine
        .execute(
            "provider_account_save",
            json!({"provider":provider,"name":"Account","apiKey":KEY}),
        )
        .unwrap()
}
fn edit(view: &Value) -> Value {
    view["account"].clone()
}
fn balance(amount: &str) -> Value {
    json!({"is_available":true,"balance_infos":[{"currency":"CNY","total_balance":amount,"granted_balance":"0.001","topped_up_balance":"5.12"}]})
}
fn quota() -> Value {
    let row = json!({"percent":12.5,"status":"ok","resetsAt":"2026-10-20T12:00:00+08:00"});
    json!({"usage":{"rolling":row,"weekly":row,"monthly":row}})
}
#[test]
fn balance_and_quota_parsers_keep_precision_and_allowlist_only_known_fields() {
    let mut value = balance("6.68249010");
    value["apiKey"] = json!(KEY);
    let parsed = parse_response("deepseek", value).unwrap();
    assert_eq!(parsed["balances"][0]["total"], "6.6824901");
    assert!(!parsed.to_string().contains(KEY));
    let parsed = parse_response("opencode-go", quota()).unwrap();
    assert_eq!(parsed["windows"].as_array().unwrap().len(), 3);
    assert_eq!(parsed["windows"][0]["usedPercent"], 12.5);
    assert_eq!(parsed["windows"][0]["resetsAt"], 1792468800i64);
}
#[test]
fn malformed_or_missing_provider_data_never_becomes_a_zero_balance_or_full_quota() {
    for bad in [
        json!({}),
        json!({"is_available":true,"balance_infos":[]}),
        json!({"is_available":false,"balance_infos":[{"currency":"USD","total_balance":"bad"}]}),
    ] {
        assert!(parse_response("deepseek", bad).is_err());
    }
    let mut zero = balance("0");
    zero["is_available"] = json!(false);
    assert_eq!(
        parse_response("deepseek", zero).unwrap()["balances"][0]["total"],
        "0"
    );
    for field in ["percent", "status", "resetsAt"] {
        let mut raw = quota();
        raw["usage"]["rolling"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(parse_response("opencode-go", raw).is_err());
    }
    for bad in [json!(-1), json!(10001), json!("0")] {
        let mut raw = quota();
        raw["usage"]["rolling"]["percent"] = bad;
        assert!(parse_response("opencode-go", raw).is_err());
    }
}
#[test]
fn credentials_are_local_persisted_redacted_and_excluded_from_database_and_sync() {
    let (directory, engine, vault) = setup();
    let initial_version = engine.lock().unwrap().usage_version().unwrap();
    let view = create(&engine, "opencode-go");
    let id = view["account"]["id"].as_str().unwrap();
    assert_eq!(view["keyConfigured"], true);
    assert_eq!(vault.read(id).unwrap(), KEY);
    #[cfg(windows)]
    assert!(!String::from_utf8_lossy(&vault.raw(id).unwrap().unwrap()).contains(KEY));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(vault.path(id).unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let list = engine.execute("provider_accounts", json!({})).unwrap();
    assert!(!list.to_string().contains(KEY));
    assert!(!engine
        .execute("snapshot", json!({}))
        .unwrap()
        .to_string()
        .contains(KEY));
    assert!(engine.lock().unwrap().pending().unwrap().is_empty());
    assert_eq!(
        engine.lock().unwrap().usage_version().unwrap(),
        initial_version
    );
    let stored: String = engine
        .lock()
        .unwrap()
        .conn
        .query_row("SELECT group_concat(value) FROM meta", [], |r| r.get(0))
        .unwrap();
    assert!(!stored.contains(KEY));
    drop(engine);
    let reopened = Engine::open(&directory.path().join("scope.db")).unwrap();
    assert_eq!(
        reopened.execute("provider_accounts", json!({})).unwrap()[0]["keyConfigured"],
        true
    );
    assert!(reopened
        .execute("provider_account_delete", json!({"id":"../outside"}))
        .is_err());
}
#[test]
fn edits_preserve_blank_keys_validate_before_writing_and_clear_explicitly() {
    let (_directory, engine, vault) = setup();
    let view = create(&engine, "deepseek");
    let id = view["account"]["id"].as_str().unwrap();
    let mut patch = edit(&view);
    patch["name"] = json!("Renamed");
    patch["apiKey"] = json!("");
    engine
        .execute("provider_account_save", patch.clone())
        .unwrap();
    assert_eq!(vault.read(id).unwrap(), KEY);
    patch["subscription"]["monthlyFee"] = json!("-1");
    patch["apiKey"] = json!("replacement");
    assert!(engine
        .execute("provider_account_save", patch.clone())
        .is_err());
    assert_eq!(vault.read(id).unwrap(), KEY);
    patch["subscription"]["monthlyFee"] = Value::Null;
    patch["subscription"]["periodStart"] = json!("2026-10-20");
    patch["subscription"]["renewalDate"] = json!("2026-10-19");
    assert!(engine
        .execute("provider_account_save", patch.clone())
        .is_err());
    assert_eq!(vault.read(id).unwrap(), KEY);
    let mut changed_provider = edit(&view);
    changed_provider["provider"] = json!("opencode-go");
    assert!(engine
        .execute("provider_account_save", changed_provider)
        .is_err());
    assert_eq!(vault.read(id).unwrap(), KEY);
    patch = edit(&view);
    patch["clearApiKey"] = json!(true);
    let saved = engine.execute("provider_account_save", patch).unwrap();
    assert_eq!(saved["keyConfigured"], false);
    assert!(!vault.exists(id));
}
#[test]
fn database_write_failure_restores_the_previous_credential() {
    let (_directory, engine, vault) = setup();
    let view = create(&engine, "deepseek");
    let id = view["account"]["id"].as_str().unwrap();
    engine.lock().unwrap().conn.execute_batch("CREATE TRIGGER reject_account_update BEFORE UPDATE ON meta WHEN NEW.key='provider_accounts_v1' BEGIN SELECT RAISE(ABORT,'simulated failure'); END;").unwrap();
    let mut patch = edit(&view);
    patch["apiKey"] = json!("replacement");
    assert!(engine.execute("provider_account_save", patch).is_err());
    assert_eq!(vault.read(id).unwrap(), KEY);
}
fn record(
    device: &str,
    id: &str,
    harness: &str,
    provider: &str,
    cost: Option<&str>,
) -> UsageRecord {
    UsageRecord {
        timing: None,
        request_metadata: None,
        device_id: device.into(),
        id: id.into(),
        provider: Some(provider.into()),
        harness: harness.into(),
        parser_version: "fixture".into(),
        session_id: Some("session".into()),
        native_event_id: Some(id.into()),
        occurred_at: Utc::now().timestamp(),
        model: "Original-Model-High".into(),
        grain: "event".into(),
        count: 1,
        tokens: Tokens {
            input: 100,
            output: 20,
            cache_read: 80,
            ..Default::default()
        },
        cost: Cost {
            amount_usd: cost.map(str::to_string),
            ..Default::default()
        },
        record_source: "session".into(),
        revision: 1,
        deleted: false,
        status_code: None,
        legacy_date: None,
        legacy_timezone: None,
    }
}
#[test]
fn linked_usage_includes_remote_routes_without_own_mirrors_or_deleted_facts() {
    let (_directory, engine, _vault) = setup();
    let view = create(&engine, "opencode-go");
    let mut patch = edit(&view);
    patch["links"] = json!([{"harness":"dsh","provider":"opencode-go"},{"harness":"opencode","provider":"opencode-go-plus"}]);
    engine
        .execute("provider_account_save", patch.clone())
        .unwrap();
    let original;
    {
        let s = engine.lock().unwrap();
        let device = s.device_id().unwrap();
        s.upsert(
            record(&device, "a", "dsh", "opencode-go", Some("0.1")),
            "fixture",
        )
        .unwrap();
        s.upsert(
            record(&device, "b", "opencode", "opencode-go-plus", Some("0.2")),
            "fixture",
        )
        .unwrap();
        s.upsert(
            record(&device, "c", "dsh", "opencode-go-plus", Some("5")),
            "fixture",
        )
        .unwrap();
        s.upsert(record(&device, "d", "dsh", "opencode-go", None), "fixture")
            .unwrap();
        let remote = record("remote", "remote", "dsh", "opencode-go", Some("99"));
        cache_remote(&s, &remote);
        cache_remote(&s, &record(&device, "a", "dsh", "opencode-go", Some("999")));
        let mut deleted = remote.clone();
        deleted.id = "deleted".into();
        deleted.deleted = true;
        cache_remote(&s, &deleted);
        cache_remote(
            &s,
            &record("remote", "unlinked", "claude", "remote-only", Some("999")),
        );
        original = s.local_records().unwrap();
    }
    let usage = engine
        .execute(
            "provider_account_usage",
            json!({"id":view["account"]["id"],"range":"today"}),
        )
        .unwrap();
    assert_eq!(usage["summary"]["totalRequests"], 4);
    assert_eq!(usage["summary"]["totalCost"], "99.3");
    assert_eq!(usage["summary"]["unpricedCount"], 1);
    assert_eq!(usage["summary"]["realTotalTokens"], 800);
    assert_eq!(usage["models"][0]["model"], "Original-Model-High");
    assert_eq!(engine.lock().unwrap().local_records().unwrap(), original);
    let other = create(&engine, "opencode-go");
    let empty = engine
        .execute(
            "provider_account_usage",
            json!({"id":other["account"]["id"],"range":"today"}),
        )
        .unwrap();
    assert_eq!(empty["summary"]["totalRequests"], 0);
    let routes = engine
        .execute("provider_account_routes", json!({}))
        .unwrap();
    let routes = routes.as_array().unwrap();
    assert_eq!(
        routes
            .iter()
            .find(|r| r["harness"] == "dsh" && r["provider"] == "opencode-go")
            .unwrap()["records"],
        3
    );
    assert_eq!(
        routes
            .iter()
            .find(|r| r["provider"] == "remote-only")
            .unwrap()["records"],
        1
    );
    let mut other_patch = edit(&other);
    other_patch["links"] = patch["links"].clone();
    assert!(engine
        .execute("provider_account_save", other_patch)
        .is_err());
    engine
        .execute(
            "provider_account_delete",
            json!({"id":view["account"]["id"]}),
        )
        .unwrap();
    assert_eq!(engine.lock().unwrap().local_records().unwrap(), original);
}

pub(super) fn cache_remote(store: &Store, row: &UsageRecord) {
    store.conn.execute(
        "INSERT OR REPLACE INTO remote_records(device_id,id,revision,payload) VALUES(?1,?2,?3,?4)",
        rusqlite::params![row.device_id,row.id,row.revision,serde_json::to_string(row).unwrap()],
    ).unwrap();
}
#[test]
fn usage_ranges_follow_configured_midnight_and_manual_renewal_boundaries() {
    let (_directory, engine, _vault) = setup();
    let view = create(&engine, "opencode-go");
    let id = view["account"]["id"].as_str().unwrap();
    let mut patch = edit(&view);
    patch["links"] = json!([{"harness":"dsh","provider":"opencode-go"}]);
    patch["subscription"]["periodStart"] = json!("2020-01-01");
    patch["subscription"]["renewalDate"] = json!("2020-02-01");
    engine.execute("provider_account_save", patch).unwrap();
    let timezone: chrono_tz::Tz = "Asia/Shanghai".parse().unwrap();
    let timestamp = |date| {
        midnight(
            timezone,
            NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
        )
        .unwrap()
    };
    let start = timestamp("2020-01-01");
    let end = timestamp("2020-02-01");
    let now = Utc::now();
    let today = midnight(timezone, now.with_timezone(&timezone).date_naive()).unwrap();
    {
        let store = engine.lock().unwrap();
        let mut config = store.config().unwrap();
        config.timezone = timezone.name().into();
        store.set_meta("config", &config).unwrap();
        let device = store.device_id().unwrap();
        for (name, time, price) in [
            ("before-period", start - 1, "99"),
            ("period-start", start, "0.1"),
            ("before-renewal", end - 1, "0.2"),
            ("renewal", end, "99"),
            ("yesterday", today - 1, "99"),
            ("today", now.timestamp(), "0.3"),
        ] {
            let mut row = record(&device, name, "dsh", "opencode-go", Some(price));
            row.occurred_at = time;
            store.upsert(row, "fixture").unwrap();
        }
    }
    let period = engine
        .execute(
            "provider_account_usage",
            json!({"id":id,"range":"subscription"}),
        )
        .unwrap();
    assert_eq!(period["startDate"], start);
    assert_eq!(period["endDate"], end - 1);
    assert_eq!(period["summary"]["totalRequests"], 2);
    assert_eq!(period["summary"]["totalCost"], "0.3");
    let usage = engine
        .execute("provider_account_usage", json!({"id":id,"range":"today"}))
        .unwrap();
    assert_eq!(usage["startDate"], today);
    assert_eq!(usage["summary"]["totalRequests"], 1);
    assert_eq!(usage["summary"]["totalCost"], "0.3");
    assert_eq!(usage["timezone"], "Asia/Shanghai");
}
#[test]
fn failed_refresh_keeps_last_good_data_and_does_not_hold_query_or_write_locks() {
    let (_directory, engine, vault) = setup();
    let view = create(&engine, "deepseek");
    let id = view["account"]["id"].as_str().unwrap();
    let success = refresh_with(&engine, &vault, id, |provider, key| {
        assert_eq!(key, KEY);
        engine
            .execute("usage_query", json!({"kind":"summary"}))
            .unwrap();
        engine.execute("snapshot", json!({})).unwrap();
        parse_response(provider, balance("5.121"))
    })
    .unwrap();
    let failed = refresh_with(&engine, &vault, id, |_, _| {
        anyhow::bail!("API key is invalid or expired (401)")
    })
    .unwrap();
    assert_eq!(success["snapshot"], failed["snapshot"]);
    assert!(failed["error"].as_str().unwrap().contains("401"));
}
#[test]
fn key_changes_and_deletions_during_a_refresh_cannot_restore_old_account_data() {
    let (_directory, engine, vault) = setup();
    let view = create(&engine, "deepseek");
    let id = view["account"]["id"].as_str().unwrap();
    let response = refresh_with(&engine, &vault, id, |provider, _| {
        let mut patch = edit(&view);
        patch["apiKey"] = json!("new-local-provider-key");
        engine.execute("provider_account_save", patch).unwrap();
        parse_response(provider, balance("99"))
    })
    .unwrap();
    assert!(response["snapshot"].is_null());
    assert_eq!(vault.read(id).unwrap(), "new-local-provider-key");
    assert!(refresh_with(&engine, &vault, id, |provider, _| {
        engine
            .execute("provider_account_delete", json!({"id":id}))
            .unwrap();
        parse_response(provider, balance("99"))
    })
    .is_err());
    assert_eq!(
        engine.execute("provider_accounts", json!({})).unwrap(),
        json!([])
    );
    assert!(!vault.exists(id));
}
#[test]
fn last_started_refresh_wins_even_if_an_older_response_arrives_late() {
    let (_directory, engine, vault) = setup();
    let view = create(&engine, "deepseek");
    let id = view["account"]["id"].as_str().unwrap();
    let response = refresh_with(&engine, &vault, id, |provider, _| {
        refresh_with(&engine, &vault, id, |provider, _| {
            parse_response(provider, balance("2"))
        })
        .unwrap();
        parse_response(provider, balance("1"))
    })
    .unwrap();
    assert_eq!(response["snapshot"]["data"]["balances"][0]["total"], "2");
}
fn server(
    status: u16,
    body: String,
    location: Option<&str>,
) -> (String, std::thread::JoinHandle<String>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let location = location.map(str::to_owned);
    let task = std::thread::spawn(move || {
        use std::io::Write;
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0u8];
            if socket.read(&mut byte).unwrap() == 0 {
                break;
            }
            request.push(byte[0]);
            assert!(request.len() <= 8192);
        }
        let response=format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n{body}",body.len(),location.map(|v|format!("Location: {v}\r\n")).unwrap_or_default());
        let _ = socket.write_all(response.as_bytes());
        String::from_utf8_lossy(&request).into_owned()
    });
    (format!("http://{address}/usage"), task)
}
#[test]
fn http_authentication_is_backend_only_and_errors_never_echo_provider_bodies() {
    let (url, task) = server(200, quota().to_string(), None);
    assert!(request("opencode-go", KEY, &url).is_ok());
    let sent = task.join().unwrap().to_lowercase();
    assert!(sent.contains(&format!("authorization: bearer {KEY}")));
    let (url, task) = server(401, format!("{{\"error\":\"{KEY}\"}}"), None);
    let error = request("opencode-go", KEY, &url).unwrap_err().to_string();
    task.join().unwrap();
    assert!(error.contains("401"));
    assert!(!error.contains(KEY));
    let (url, task) = server(302, "{}".into(), Some("http://127.0.0.1:1/steal"));
    assert!(request("opencode-go", KEY, &url)
        .unwrap_err()
        .to_string()
        .contains("302"));
    task.join().unwrap();
    let (url, task) = server(200, " ".repeat(140000), None);
    assert!(request("opencode-go", KEY, &url)
        .unwrap_err()
        .to_string()
        .contains("too large"));
    task.join().unwrap();
}

#[test]
fn legacy_accounts_gain_refresh_defaults_without_replacing_manual_subscription_fields() {
    let (_directory, engine, vault) = setup();
    let view = create(&engine, "opencode-go");
    let id = view["account"]["id"].as_str().unwrap();
    let subscription = json!({"planName":"Go Plus","monthlyFee":"10","currency":"USD","periodStart":"2026-09-28","renewalDate":"2026-10-28"});
    let mut patch = edit(&view);
    patch["subscription"] = subscription.clone();
    engine.execute("provider_account_save", patch).unwrap();
    {
        let store = engine.lock().unwrap();
        let mut legacy: Value = store.meta(META).unwrap().unwrap();
        legacy[0]["account"]
            .as_object_mut()
            .unwrap()
            .remove("refresh");
        store.set_meta(META, &legacy).unwrap();
    }
    let refreshed = polling::refresh_managed(&engine, &vault, id, |provider, _| {
        let mut raw = quota();
        raw["usage"]["monthly"]["resetsAt"] = json!("2026-10-29T11:37:00Z");
        raw["subscription"] = json!({"planName":"Do not trust undocumented fields","monthlyFee":"999","renewalDate":"2026-12-01"});
        parse_response(provider, raw)
    }).unwrap();
    assert_eq!(
        refreshed["account"]["refresh"],
        json!({"foregroundSeconds":60,"backgroundSeconds":300})
    );
    assert_eq!(refreshed["account"]["subscription"], subscription);
    assert!(refreshed["snapshot"]["data"].get("subscription").is_none());
    assert_eq!(refreshed["refreshing"], false);
}

#[test]
fn refresh_policy_validates_atomically_supports_disabling_and_keeps_usage_unchanged() {
    let (_directory, engine, _vault) = setup();
    let initial = create(&engine, "opencode-go");
    let version = engine.lock().unwrap().usage_version().unwrap();
    for invalid in [json!(1), json!(14), json!(86401), json!(-1), json!(15.5)] {
        let mut patch = edit(&initial);
        patch["refresh"]["foregroundSeconds"] = invalid;
        assert!(engine.execute("provider_account_save", patch).is_err());
        assert_eq!(
            engine.execute("provider_accounts", json!({})).unwrap()[0],
            initial
        );
    }
    let mut patch = edit(&initial);
    patch["refresh"] = json!({"foregroundSeconds":0,"backgroundSeconds":0});
    engine.execute("provider_account_save", patch).unwrap();
    assert!(polling::due_at(
        &engine,
        &engine.account_vault,
        i64::MAX,
        std::time::Instant::now()
    )
    .unwrap()
    .is_empty());
    let id = initial["account"]["id"].as_str().unwrap();
    let manual = polling::refresh_managed(&engine, &engine.account_vault, id, |provider, _| {
        parse_response(provider, quota())
    })
    .unwrap();
    assert!(manual["snapshot"].is_object());
    assert_eq!(engine.lock().unwrap().usage_version().unwrap(), version);
    assert!(engine.lock().unwrap().pending().unwrap().is_empty());
}

#[test]
fn foreground_leases_expire_and_old_page_cleanup_does_not_deactivate_new_page() {
    use std::time::Instant;
    let (_directory, engine, vault) = setup();
    let go = create(&engine, "opencode-go");
    let ds = create(&engine, "deepseek");
    {
        let store = engine.lock().unwrap();
        let mut items = load(&store).unwrap();
        for item in &mut items {
            item.last_attempt_at = Some(1000);
        }
        store.set_meta(META, &items).unwrap();
    }
    let id = go["account"]["id"].as_str().unwrap();
    let first = uuid::Uuid::new_v4().to_string();
    let second = uuid::Uuid::new_v4().to_string();
    for lease in [&first, &second] {
        engine
            .execute(
                "provider_account_activity",
                json!({"id":id,"lease":lease,"active":true}),
            )
            .unwrap();
    }
    engine
        .execute(
            "provider_account_activity",
            json!({"id":id,"lease":first,"active":false}),
        )
        .unwrap();
    let clock = Instant::now();
    assert!(polling::due_at(&engine, &vault, 1059, clock)
        .unwrap()
        .is_empty());
    assert_eq!(
        polling::due_at(&engine, &vault, 1060, clock).unwrap(),
        vec![id]
    );
    assert!(
        polling::due_at(&engine, &vault, 1060, clock + Duration::from_secs(46))
            .unwrap()
            .is_empty()
    );
    let due = polling::due_at(&engine, &vault, 1300, clock + Duration::from_secs(46)).unwrap();
    assert_eq!(due.len(), 2);
    assert!(due.contains(&ds["account"]["id"].as_str().unwrap().to_owned()));
}

#[test]
fn shared_refresh_guard_deduplicates_manual_and_scheduled_requests_and_releases_on_failure() {
    let (_directory, engine, vault) = setup();
    let initial = create(&engine, "opencode-go");
    let id = initial["account"]["id"].as_str().unwrap();
    let result = polling::refresh_managed(&engine, &vault, id, |_, _| {
        assert!(polling::due_at(
            &engine,
            &vault,
            Utc::now().timestamp(),
            std::time::Instant::now()
        )
        .unwrap()
        .is_empty());
        let overlap = polling::refresh_managed(&engine, &vault, id, |_, _| {
            panic!("Duplicate network request")
        })
        .unwrap();
        assert_eq!(overlap["refreshing"], true);
        assert_eq!(
            engine.execute("provider_accounts", json!({})).unwrap()[0]["refreshing"],
            true
        );
        anyhow::bail!("simulated transient failure")
    })
    .unwrap();
    assert_eq!(result["refreshing"], false);
    let next = polling::refresh_managed(&engine, &vault, id, |provider, _| {
        parse_response(provider, quota())
    })
    .unwrap();
    assert!(next["error"].is_null());
}

#[test]
fn background_tick_fetches_without_ui_and_observes_cooldown_and_error_backoff() {
    let (_directory, engine, vault) = setup();
    let view = create(&engine, "opencode-go");
    let id = view["account"]["id"].as_str().unwrap().to_owned();
    let engine = std::sync::Arc::new(engine);
    assert_eq!(
        polling::tick(&engine, |p, _| parse_response(p, quota())).unwrap(),
        1
    );
    assert_eq!(
        polling::tick(&engine, |_, _| panic!("duplicate tick")).unwrap(),
        0
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let list = engine.execute("provider_accounts", json!({})).unwrap();
        if list[0]["refreshing"] == false && list[0]["snapshot"].is_object() {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(
        polling::tick(&engine, |_, _| panic!("cooldown ignored")).unwrap(),
        0
    );
    {
        let store = engine.lock().unwrap();
        let mut items = load(&store).unwrap();
        items[0].account.refresh = RefreshPolicy {
            foreground_seconds: 15,
            background_seconds: 15,
        };
        items[0].last_attempt_at = Some(1000);
        items[0].error = Some("Provider rate limit reached (429)".into());
        store.set_meta(META, &items).unwrap();
    }
    assert!(
        polling::due_at(&engine, &vault, 1015, std::time::Instant::now())
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        polling::due_at(&engine, &vault, 1060, std::time::Instant::now()).unwrap(),
        vec![id]
    );
}

#[test]
fn unreadable_credentials_preserve_cache_report_failure_and_back_off() {
    let (_directory, engine, vault) = setup();
    let initial = create(&engine, "opencode-go");
    let id = initial["account"]["id"].as_str().unwrap();
    let mut patch = edit(&initial);
    patch["refresh"] = json!({"foregroundSeconds":15,"backgroundSeconds":15});
    engine.execute("provider_account_save", patch).unwrap();
    let cached =
        polling::refresh_managed(&engine, &vault, id, |p, _| parse_response(p, quota())).unwrap();
    vault.restore(id, Some(&[255])).unwrap();
    let failed = polling::refresh_managed(&engine, &vault, id, |_, _| {
        panic!("Unreadable credential sent upstream")
    })
    .unwrap();
    assert!(failed["error"].is_string());
    assert_eq!(failed["snapshot"], cached["snapshot"]);
    assert_eq!(failed["refreshing"], false);
    let attempt = failed["lastAttemptAt"].as_i64().unwrap();
    assert!(
        polling::due_at(&engine, &vault, attempt + 15, std::time::Instant::now())
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        polling::due_at(&engine, &vault, attempt + 60, std::time::Instant::now()).unwrap(),
        vec![id]
    );
}

#[test]
fn go_estimates_sum_model_normalized_allowances_without_repricing_facts() {
    let (_directory, engine, _vault) = setup();
    let view = create(&engine, "opencode-go");
    let id = view["account"]["id"].as_str().unwrap();
    let mut patch = edit(&view);
    patch["links"] = json!([{"harness":"dsh","provider":"opencode-go"}]);
    engine.execute("provider_account_save", patch).unwrap();
    let info = engine.execute("go_catalog", json!({})).unwrap();
    let mut catalog = crate::pricing::go_catalog::defaults();
    for (id, limit) in [("deepseek-flash", "10"), ("deepseek-v4-pro", "100")] {
        let m = catalog.models.iter_mut().find(|m| m.id == id).unwrap();
        m.tiers[0].prices.input = "1".into();
        m.tiers[0].peak_prices.as_mut().unwrap().input = "1".into();
        m.tiers[0].monthly_limits.go = Some(limit.into());
        m.tiers[0].monthly_limits.go_plus = Some(
            (rust_decimal::Decimal::from_str(limit).unwrap() * rust_decimal::Decimal::TWO)
                .to_string(),
        );
    }
    engine.execute("save_go_catalog",json!({"catalog":catalog,"expectedRevision":info["revision"],"expectedFileRevision":null})).unwrap();
    let now = Utc::now().timestamp();
    {
        let store = engine.lock().unwrap();
        let device = store.device_id().unwrap();
        for (key, model, provider) in [
            ("flash", "opencode/DeepSeek-V4.1-Flash-high", "opencode-go"),
            ("pro", "deepseek-v4-pro", "opencode-go"),
            ("free", "beam-free", "opencode-go"),
            ("unknown", "missing-model", "opencode-go"),
            ("write", "deepseek-flash", "opencode-go"),
            ("unlinked", "deepseek-v4-pro", "other-route"),
        ] {
            let mut fact = record(&device, key, "dsh", provider, Some("999"));
            fact.model = model.into();
            fact.occurred_at = now - 1;
            fact.tokens = Tokens {
                input: 1_000_000,
                ..Default::default()
            };
            if key == "write" {
                fact.tokens.cache_write = 1;
            }
            if key == "pro" {
                fact.device_id = "remote".into();
                cache_remote(&store, &fact);
            } else {
                store.upsert(fact, "fixture").unwrap();
            }
        }
        let mut items = load(&store).unwrap();
        items[0].snapshot = Some(
            json!({"fetchedAt":now,"data":{"kind":"quota","windows":[{"id":"rolling","resetsAt":now+300,"status":"ok","usedPercent":5},{"id":"weekly","resetsAt":now+300,"status":"ok","usedPercent":5},{"id":"monthly","resetsAt":now+300,"status":"ok","usedPercent":5}]}}),
        );
        store.set_meta(META, &items).unwrap();
    }
    let before = engine.lock().unwrap().local_records().unwrap();
    let version = engine.lock().unwrap().usage_version().unwrap();
    let pending = engine.lock().unwrap().pending().unwrap();
    let usage = engine
        .execute("go_usage_estimate", json!({"id":id,"plan":"go"}))
        .unwrap();
    for (i, percent) in [(0, "55"), (1, "22"), (2, "11")] {
        let w = &usage["windows"][i];
        assert_eq!(w["total"]["percent"], percent);
        assert_eq!(w["total"]["requests"], 5);
        assert_eq!(w["total"]["unmatchedRequests"], 2);
        assert_eq!(w["total"]["costUsd"], "2");
        assert_eq!(w["basis"], "provider_window");
    }
    assert!(usage["windows"][2]["models"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["model"] == "opencode/DeepSeek-V4.1-Flash-high"
            && v["pricingModel"] == "deepseek-flash"));
    let plus = engine
        .execute("go_usage_estimate", json!({"id":id,"plan":"goPlus"}))
        .unwrap();
    assert_eq!(plus["windows"][2]["total"]["percent"], "5.5");
    let simulated=engine.execute("go_simulate",json!({"model":"deepseek-v4.1-flash","plan":"go","requests":2,"tokens":{"input":1_000_000,"output":0,"cacheRead":0,"cacheWrite":0},"period":"peak"})).unwrap();
    assert_eq!(simulated["percent"]["monthly"], "20");
    assert_eq!(simulated["percent"]["rolling"], "100");
    assert_eq!(engine.lock().unwrap().local_records().unwrap(), before);
    assert_eq!(engine.lock().unwrap().usage_version().unwrap(), version);
    assert_eq!(engine.lock().unwrap().pending().unwrap(), pending);
}
