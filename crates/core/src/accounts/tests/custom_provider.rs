use super::*;
use std::{io::Write, net::TcpListener, sync::mpsc, thread};

fn settings(url: &str, extractor: &str) -> custom::Settings {
    custom::Settings {
        enabled: true,
        base_url: url.into(),
        timeout_seconds: 2,
        script: format!(
            r#"({{request: {{url: "{{{{baseUrl}}}}/usage", headers: {{Authorization: "Bearer {{{{apiKey}}}}"}}}}, extractor(response) {{ {extractor} }} }})"#
        ),
    }
}
fn draft(name: &str) -> Value {
    json!({"provider":"custom","name":name,"custom":{"enabled":false,"baseUrl":"","script":"","timeoutSeconds":10},"refresh":{"foregroundSeconds":0,"backgroundSeconds":0}})
}
fn server(status: &str, body: &str) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (send, receive) = mpsc::channel();
    let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        loop {
            let mut buffer = [0; 4096];
            let n = stream.read(&mut buffer).unwrap();
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..n]);
            if let Some(pos) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let header = String::from_utf8_lossy(&bytes[..pos]).to_ascii_lowercase();
                let len = header
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length: "))
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(0);
                if bytes.len() >= pos + 4 + len {
                    break;
                }
            }
        }
        send.send(String::from_utf8(bytes).unwrap()).unwrap();
        stream.write_all(response.as_bytes()).unwrap();
    });
    (url, receive, handle)
}
fn direct() -> crate::network::Settings {
    crate::network::Settings {
        mode: crate::network::Mode::Direct,
        proxy_url: String::new(),
    }
}

#[test]
fn multiple_custom_providers_keep_identity_exact_routes_and_device_facts() {
    let (_dir, engine, vault) = setup();
    let mut a = draft("Relay A");
    a["links"] = json!([{"harness":"codex","provider":"my-relay"}]);
    let a = engine.execute("provider_account_save", a).unwrap();
    let b = engine
        .execute("provider_account_save", draft("Relay B"))
        .unwrap();
    assert_ne!(a["account"]["provider"], b["account"]["provider"]);
    assert_eq!(a["canRefresh"], false);
    assert!(!vault.exists(a["account"]["id"].as_str().unwrap()));
    assert!(
        polling::due_at(&engine, &vault, i64::MAX, std::time::Instant::now())
            .unwrap()
            .is_empty()
    );
    let mut conflict = edit(&b);
    conflict["links"] = a["account"]["links"].clone();
    assert!(engine
        .execute("provider_account_save", conflict.clone())
        .is_err());
    conflict["links"][0]["harness"] = json!("opencode");
    engine.execute("provider_account_save", conflict).unwrap();
    let mut renamed = edit(&a);
    renamed["name"] = json!("Renamed");
    let renamed = engine.execute("provider_account_save", renamed).unwrap();
    assert_eq!(renamed["account"]["provider"], a["account"]["provider"]);
    let mut forged = edit(&renamed);
    forged["provider"] = b["account"]["provider"].clone();
    assert!(engine.execute("provider_account_save", forged).is_err());
    let store = engine.lock().unwrap();
    assert_eq!(
        store
            .conn
            .query_row("SELECT COUNT(*) FROM records", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        store
            .conn
            .query_row("SELECT COUNT(*) FROM outbox", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn query_uses_proxy_expands_credentials_as_data_and_preserves_decimal_strings() {
    let (proxy, received, thread) = server("200 OK", r#"{"balance":"12.34567890123456789"}"#);
    let key = r#"secret-quote\"-do-not-eval"#;
    let mut config = settings(
        "https://usage.example.invalid",
        "return {remaining: response.balance, unit: 'USD'};",
    );
    config.script = config.script.replace(
        "headers: {",
        "method: 'POST', body: { token: '{{apiKey}}' }, headers: {",
    );
    // An HTTP target allows the fixture to observe the request without TLS; HTTPS
    // CONNECT and SOCKS are covered by the shared client's network tests.
    config.base_url = "http://usage.example.invalid".into();
    let network = crate::network::Settings {
        mode: crate::network::Mode::Manual,
        proxy_url: proxy,
    };
    let result = custom::request(&config, key, &network).unwrap();
    let sent = received.recv().unwrap();
    thread.join().unwrap();
    assert!(sent.starts_with("POST http://usage.example.invalid/usage "));
    assert!(sent.contains(&format!("Bearer {key}")));
    let body: Value = serde_json::from_str(sent.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(body["token"], key);
    assert_eq!(result["items"][0]["remaining"], "12.34567890123456789");
    assert!(!result.to_string().contains(key));
}

#[test]
fn scripts_are_bounded_and_cannot_retarget_requests_or_use_host_apis() {
    for code in [
        "(()=>{ while(true) {} })()",
        "process.exit()",
        "require('fs')",
        "fetch('http://example.com')",
        "({request:{url:'http://other.invalid'},extractor(){return {remaining:0}}})",
    ] {
        let mut config = settings("http://127.0.0.1:1", "return {}; ");
        config.script = code.into();
        let start = std::time::Instant::now();
        assert!(custom::request(&config, "", &direct()).is_err());
        assert!(start.elapsed() < Duration::from_secs(3));
    }
    let (url, received, thread) = server(
        "200 OK",
        r#"{"secret":"test-local-provider-secret-do-not-echo"}"#,
    );
    let config = settings(&url, "return { remaining: 1, planName: response.secret }; ");
    let error = custom::request(&config, KEY, &direct()).unwrap_err();
    received.recv().unwrap();
    thread.join().unwrap();
    assert!(!error.to_string().contains(KEY));
    let (url, received, thread) = server("401 Unauthorized", KEY);
    let error = custom::request(&settings(&url, "return {};"), KEY, &direct()).unwrap_err();
    received.recv().unwrap();
    thread.join().unwrap();
    assert!(error.to_string().contains("401"));
    assert!(!error.to_string().contains(KEY));
}

#[test]
fn invalid_or_missing_metrics_never_become_free_balance_or_full_quota() {
    for value in [
        json!({}),
        json!([]),
        json!({"remaining":"NaN"}),
        json!({"total":-1}),
        json!({"remaining":5,"isValid":false}),
        json!({"usedPercent":-1}),
        json!({"remaining":1,"resetsAt":"2026-10-22T10:00:00"}),
        json!([{"remaining":1},{"remaining":2}]),
        json!([{"id":"x","remaining":1},{"id":"x","remaining":2}]),
    ] {
        assert!(custom::normalize(value).is_err());
    }
    let data = custom::normalize(json!({"remaining":"-0.01","unit":"CNY"})).unwrap();
    assert_eq!(data["items"][0]["remaining"], "-0.01");
    assert!(data["items"][0]["usedPercent"].is_null());
    let data = custom::normalize(json!({"total":"100","used":"12.5","unit":"requests"})).unwrap();
    assert_eq!(data["items"][0]["remaining"], "87.5");
    assert_eq!(data["items"][0]["usedPercent"], "12.5");
}

#[test]
fn test_is_read_only_real_refresh_records_history_and_only_fixed_windows_get_anchors() {
    let (_dir, engine, vault) = setup();
    engine
        .execute(
            "save_network_settings",
            json!({"settings":{"mode":"direct","proxyUrl":""}}),
        )
        .unwrap();
    let (url, received, thread) = server("200 OK", r#"{"balance":"42.001"}"#);
    let config = settings(&url, "return {remaining: response.balance, unit:'USD'};");
    let tested = engine
        .execute(
            "provider_account_test_query",
            json!({"settings":config,"apiKey":KEY}),
        )
        .unwrap();
    received.recv().unwrap();
    thread.join().unwrap();
    assert_eq!(tested["items"][0]["remaining"], "42.001");
    assert!(engine
        .execute("provider_accounts", json!({}))
        .unwrap()
        .as_array()
        .unwrap()
        .is_empty());
    let (url, received, thread) = server("200 OK", r#"{"balance":"42.001"}"#);
    let mut input = draft("Query provider");
    input["custom"] = json!(settings(
        &url,
        "return {remaining: response.balance, unit:'USD'};"
    ));
    input["apiKey"] = json!(KEY);
    let saved = engine.execute("provider_account_save", input).unwrap();
    let id = saved["account"]["id"].as_str().unwrap();
    let refreshed = engine
        .execute("provider_account_refresh", json!({"id":id}))
        .unwrap();
    received.recv().unwrap();
    thread.join().unwrap();
    assert!(refreshed["error"].is_null());
    let hist = engine
        .execute("provider_account_history", json!({"id":id}))
        .unwrap();
    assert_eq!(hist["observations"].as_array().unwrap().len(), 1);
    assert_eq!(hist["observations"][0]["data"], tested);
    let reset = Utc::now().timestamp() + 1800;
    let data = custom::normalize(json!([
        {"id":"fixed","usedPercent":"20","durationMinutes":60,"resetsAt":reset,"windowType":"fixed"},
        {"id":"rolling","usedPercent":"20","durationMinutes":60,"resetsAt":reset,"windowType":"rolling"}
    ])).unwrap();
    refresh_with(&engine, &vault, id, |_, _| Ok(data)).unwrap();
    let hist = engine
        .execute("provider_account_history", json!({"id":id}))
        .unwrap();
    assert_eq!(
        hist["observations"][0]["windows"][0]["fullQuotaAnchor"]["at"],
        reset - 3600
    );
    assert_eq!(
        hist["observations"][0]["windows"][0]["durationSource"],
        "customQuery"
    );
    assert!(hist["observations"][0]["windows"][1]["fullQuotaAnchor"].is_null());
    let mut changed = edit(&saved);
    changed["custom"]["enabled"] = json!(false);
    let changed = engine.execute("provider_account_save", changed).unwrap();
    assert!(changed["snapshot"].is_null());
    assert!(!can_refresh(
        &serde_json::from_value(changed["account"].clone()).unwrap(),
        &vault
    ));
}

#[test]
fn session_attribution_is_local_exact_and_does_not_rewrite_original_facts() {
    let (_dir, engine, _) = setup();
    let mut input = draft("Relay");
    input["links"] = json!([{"harness":"codex","provider":"route-a"}]);
    engine.execute("provider_account_save", input).unwrap();
    let original = {
        let store = engine.lock().unwrap();
        let mut a = record(
            &store.device_id().unwrap(),
            "custom-a",
            "codex",
            "route-a",
            Some("0.12"),
        );
        a.session_id = Some("s1".into());
        store.upsert(a.clone(), "fixture").unwrap();
        let mut b = a.clone();
        b.id = "custom-b".into();
        b.provider = Some("route-b".into());
        store.upsert(b, "fixture").unwrap();
        serde_json::to_value(a).unwrap()
    };
    let rows = engine
        .execute(
            "provider_session_sources",
            json!({"harness":"codex","sessionId":"s1"}),
        )
        .unwrap();
    assert_eq!(rows[0]["providerName"], "Relay");
    assert_eq!(rows[0]["providerId"], "route-a");
    assert!(rows[1]["providerName"].is_null());
    assert!(engine
        .execute(
            "provider_session_sources",
            json!({"harness":"opencode","sessionId":"s1"})
        )
        .unwrap()
        .as_array()
        .unwrap()
        .is_empty());
    let store = engine.lock().unwrap();
    let payload: String = store
        .conn
        .query_row("SELECT payload FROM records WHERE id='custom-a'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(serde_json::from_str::<Value>(&payload).unwrap(), original);
}
