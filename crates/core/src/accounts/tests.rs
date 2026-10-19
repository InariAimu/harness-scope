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
fn local_usage_links_exact_routes_excludes_remote_facts_and_keeps_decimal_costs() {
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
        s.conn
            .execute(
                "INSERT INTO remote_records VALUES(?1,?2,1,?3)",
                rusqlite::params![
                    remote.device_id,
                    remote.id,
                    serde_json::to_string(&remote).unwrap()
                ],
            )
            .unwrap();
        original = s.local_records().unwrap();
    }
    let usage = engine
        .execute(
            "provider_account_usage",
            json!({"id":view["account"]["id"],"range":"today"}),
        )
        .unwrap();
    assert_eq!(usage["summary"]["totalRequests"], 3);
    assert_eq!(usage["summary"]["totalCost"], "0.3");
    assert_eq!(usage["summary"]["unpricedCount"], 1);
    assert_eq!(usage["summary"]["realTotalTokens"], 600);
    assert_eq!(usage["models"][0]["model"], "Original-Model-High");
    assert_eq!(engine.lock().unwrap().local_records().unwrap(), original);
    let other = create(&engine, "opencode-go");
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
