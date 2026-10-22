use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use harness_scope_core::{model::Source, Engine};
use harness_scope_service::{local_router, server_router};
use serde_json::json;
use std::{fs, sync::Arc};
use tower::ServiceExt;
const TOKEN: &str = "synthetic-test-token-at-least-24-characters";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn claude_capability_defers_only_unsupported_revisions_and_resumes_without_loss() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = tempfile::tempdir().unwrap();
    let server = Arc::new(Engine::open(&dir.path().join("server.db")).unwrap());
    let capable = Arc::new(AtomicBool::new(false));
    let flag = capable.clone();
    let router = axum::Router::new().route("/health", axum::routing::get(move || {
        let flag = flag.clone();
        async move { axum::Json(json!({"schemaVersion":if flag.load(Ordering::SeqCst){Some(4)}else{None},"protocolVersion":4,"recordCapabilities":if flag.load(Ordering::SeqCst){vec!["claude_request_timing_v2"]}else{vec![]}})) }
    })).fallback_service(server_router(server.clone(), TOKEN.into()).unwrap());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let root = dir.path().join("logs");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("fixture.jsonl"),
        include_str!("../../tests/fixtures/claude/modern.jsonl"),
    )
    .unwrap();
    let client = Arc::new(Engine::open(&dir.path().join("client.db")).unwrap());
    {
        let s = client.lock().unwrap();
        let mut config = s.config().unwrap();
        config.sources = vec![Source {
            id: "fixture".into(),
            harness: "claude".into(),
            root: root.to_string_lossy().into(),
            enabled: true,
        }];
        config.server_url = format!("http://{address}");
        config.server_token = TOKEN.into();
        s.set_meta("config", &config).unwrap();
    }
    let run = client.clone();
    tokio::task::spawn_blocking(move || {
        run.scan(false).unwrap();
        let s = run.lock().unwrap();
        let mut compatible = s.local_records().unwrap().remove(0);
        compatible.id = "codex:compatible".into();
        compatible.harness = "codex".into();
        compatible.timing = None;
        compatible.request_metadata = None;
        s.upsert(compatible, "synthetic").unwrap();
        drop(s);
        let report = run.sync().unwrap();
        assert_eq!(report["uploaded"], 1);
        assert_eq!(report["deferredClaude"], 2);
        assert!(report["warning"]
            .as_str()
            .unwrap()
            .contains("retained locally"));
        assert_eq!(run.lock().unwrap().pending_count().unwrap(), 2);
        let snapshot = run.execute("snapshot", json!({})).unwrap();
        assert_eq!(snapshot["localSchemaVersion"], 6);
        assert!(snapshot["remoteSchemaVersion"].is_null());
    })
    .await
    .unwrap();
    assert_eq!(
        server
            .lock()
            .unwrap()
            .conn
            .query_row("SELECT COUNT(*) FROM server_records", [], |r| r
                .get::<_, u64>(0))
            .unwrap(),
        1
    );
    capable.store(true, Ordering::SeqCst);
    let run = client.clone();
    tokio::task::spawn_blocking(move || {
        let report = run.sync().unwrap();
        assert_eq!(report["uploaded"], 2);
        assert!(report["warning"].is_null());
        assert_eq!(run.lock().unwrap().pending_count().unwrap(), 0);
        let snapshot = run.execute("snapshot", json!({})).unwrap();
        assert_eq!(snapshot["remoteSchemaVersion"], 4);
        assert!(snapshot["remoteSchemaCheckedAt"].as_i64().unwrap() > 0);
        assert!(run
            .lock()
            .unwrap()
            .meta::<Option<String>>("last_sync_error")
            .unwrap()
            .flatten()
            .is_none());
    })
    .await
    .unwrap();
    assert_eq!(
        server
            .lock()
            .unwrap()
            .conn
            .query_row("SELECT COUNT(*) FROM server_records", [], |r| r
                .get::<_, u64>(0))
            .unwrap(),
        3
    );
    let reopened = Engine::open(&dir.path().join("client.db")).unwrap();
    assert_eq!(
        reopened.execute("snapshot", json!({})).unwrap()["remoteSchemaVersion"],
        4
    );
    let mut config = reopened.lock().unwrap().config().unwrap();
    config.server_url = "http://127.0.0.1:1".into();
    reopened
        .execute("save_config", json!({"config":config}))
        .unwrap();
    let snapshot = reopened.execute("snapshot", json!({})).unwrap();
    assert!(snapshot["remoteSchemaVersion"].is_null());
    assert!(snapshot["remoteSchemaCheckedAt"].is_null());
    handle.abort();
}

#[tokio::test]
async fn health_reports_database_schema_separately_from_protocol() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Arc::new(Engine::open(&dir.path().join("server.db")).unwrap());
    // Read the database value; do not accidentally substitute protocolVersion.
    engine
        .lock()
        .unwrap()
        .conn
        .pragma_update(None, "user_version", 3)
        .unwrap();
    let router = server_router(engine, TOKEN.into()).unwrap();
    let response = router
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let health: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(health["schemaVersion"], 3);
    assert_eq!(health["protocolVersion"], 4);
    assert!(health["recordCapabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("claude_request_timing_v2")));
}

fn gzip(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

#[tokio::test]
async fn gzip_upload_download_timing_and_decoded_body_limits() {
    use harness_scope_core::model::{Cost, Tokens};
    use std::io::Read;
    let dir = tempfile::tempdir().unwrap();
    let engine = Arc::new(Engine::open(&dir.path().join("server.db")).unwrap());
    let server = server_router(engine.clone(), TOKEN.into()).unwrap();
    let record = json!({"deviceId":"source","id":"request","harness":"codex","parserVersion":"fixture","sessionId":"session","nativeEventId":"request","occurredAt":1792500000,"model":"model","grain":"event","count":1,"tokens":Tokens::default(),"cost":Cost::default(),"recordSource":"session","revision":1,"deleted":false,"statusCode":null,"legacyDate":null,"legacyTimezone":null,"timing":{"durationMs":4000,"firstTokenMs":1000,"durationSource":"codex_token_count","firstTokenSource":"codex_item_started_at_ms"}});
    let batch = json!({"protocolVersion":4,"deviceId":"source","deviceName":"source","records":[record.clone()]});
    let body = gzip(&serde_json::to_vec(&batch).unwrap());
    let upload = || {
        Request::builder()
            .method("POST")
            .uri("/v1/push")
            .header("Authorization", format!("Bearer {TOKEN}"))
            .header("Content-Type", "application/json")
            .header("Content-Encoding", "gzip")
            .body(Body::from(body.clone()))
            .unwrap()
    };
    for _ in 0..2 {
        let response = server.clone().oneshot(upload()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    let response = server
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/changes?after=0&limit=500&protocolVersion=4")
                .header("Authorization", format!("Bearer {TOKEN}"))
                .header("Accept-Encoding", "gzip")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["Content-Encoding"], "gzip");
    assert!(response.headers()["Vary"]
        .to_str()
        .unwrap()
        .to_ascii_lowercase()
        .contains("accept-encoding"));
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let mut decoded = Vec::new();
    flate2::read::GzDecoder::new(bytes.as_ref())
        .read_to_end(&mut decoded)
        .unwrap();
    let data: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    assert_eq!(data["changes"].as_array().unwrap().len(), 1);
    assert_eq!(data["changes"][0]["record"], record);
    let response = server
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/changes?after=0&protocolVersion=3")
                .header("Authorization", format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let mut oversized = vec![b' '; 5 * 1024 * 1024];
    oversized.extend_from_slice(b"{}");
    for (encoding, payload, status) in [
        ("gzip", gzip(&oversized), StatusCode::PAYLOAD_TOO_LARGE),
        ("br", vec![1, 2, 3], StatusCode::UNSUPPORTED_MEDIA_TYPE),
    ] {
        let response = server
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/push")
                    .header("Authorization", format!("Bearer {TOKEN}"))
                    .header("Content-Type", "application/json")
                    .header("Content-Encoding", encoding)
                    .body(Body::from(payload))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }
    let response = server
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/push")
                .header("Authorization", format!("Bearer {TOKEN}"))
                .header("Content-Type", "application/json")
                .header("Content-Encoding", "gzip")
                .body(Body::from("not gzip"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(!response.status().is_success());
    assert_eq!(
        harness_scope_core::sync::changes(&engine.lock().unwrap(), 0, 500)
            .unwrap()
            .changes
            .len(),
        1
    );
}
#[tokio::test]
async fn metadata_download_requires_v3_and_preserves_pagination_and_private_field_rejection() {
    use harness_scope_core::{
        model::{Cost, PushRequest, Tokens, UsageRecord},
        sync,
    };
    let dir = tempfile::tempdir().unwrap();
    let engine = Arc::new(Engine::open(&dir.path().join("server.db")).unwrap());
    let record:UsageRecord=serde_json::from_value(json!({"deviceId":"fixture-device","id":"request","harness":"codex","parserVersion":"fixture","sessionId":"session","nativeEventId":"request","occurredAt":1792500000,"model":"gpt-6.1-sol","grain":"event","count":1,"tokens":Tokens::default(),"cost":Cost::default(),"recordSource":"session","revision":1,"deleted":false,"statusCode":null,"legacyDate":null,"legacyTimezone":null,"requestMetadata":{"serviceTier":"priority","serviceTierSource":"thread_settings","reasoningEffort":"xhigh"}})).unwrap();
    sync::push(
        &engine.lock().unwrap(),
        PushRequest {
            protocol_version: 3,
            device_id: record.device_id.clone(),
            device_name: "fixture".into(),
            records: vec![record.clone()],
        },
    )
    .unwrap();
    let server = server_router(engine, TOKEN.into()).unwrap();
    for (uri, status) in [
        ("/v1/changes?after=0&limit=1", StatusCode::BAD_REQUEST),
        (
            "/v1/changes?after=0&limit=1&protocolVersion=2",
            StatusCode::BAD_REQUEST,
        ),
        (
            "/v1/changes?after=0&limit=1&protocolVersion=3",
            StatusCode::OK,
        ),
    ] {
        let response = server
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header("Authorization", format!("Bearer {TOKEN}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        if status == StatusCode::OK {
            let data: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(response.into_body(), 1024 * 1024)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(
                data["changes"][0]["record"],
                serde_json::to_value(&record).unwrap()
            );
            assert_eq!(data["nextCursor"], 1);
            assert_eq!(data["hasMore"], false);
        }
    }
    let mut invalid = serde_json::to_value(record).unwrap();
    invalid["requestMetadata"]["apiKey"] = json!("synthetic-private-value");
    assert!(serde_json::from_value::<UsageRecord>(invalid).is_err());
}
#[tokio::test]
async fn authenticated_server_and_local_origin_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    let e = Arc::new(Engine::open(&dir.path().join("s.db")).unwrap());
    let server = server_router(e.clone(), TOKEN.into()).unwrap();
    let r = server
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/changes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    let r = server
        .oneshot(
            Request::builder()
                .uri("/v1/changes?after=0&limit=500")
                .header("Authorization", format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let local = local_router(e);
    let r = local
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/command")
                .header("content-type", "application/json")
                .header("origin", "https://unrelated.example")
                .body(Body::from(r#"{"name":"snapshot","args":{}}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn v4_engines_sync_timing_updates_and_reset_remote_timing_without_losing_local_samples() {
    use harness_scope_core::model::{Cost, Tokens, UsageRecord, UsageTiming};
    let dir = tempfile::tempdir().unwrap();
    let server = Arc::new(Engine::open(&dir.path().join("server.db")).unwrap());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = server_router(server.clone(), TOKEN.into()).unwrap();
    let handle = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut clients = Vec::new();
    for index in 0..2 {
        let e = Arc::new(Engine::open(&dir.path().join(format!("client{index}.db"))).unwrap());
        let s = e.lock().unwrap();
        let device = s.device_id().unwrap();
        let mut c = s.config().unwrap();
        c.sources.clear();
        c.server_url = format!("http://{address}");
        c.server_token = TOKEN.into();
        s.set_meta("config", &c).unwrap();
        let r:UsageRecord=serde_json::from_value(json!({"deviceId":device,"id":"same-native-id","harness":"codex","parserVersion":"fixture","sessionId":"session","nativeEventId":"same-native-id","occurredAt":1792500000,"model":"model","grain":"event","count":1,"tokens":Tokens{output:300,..Default::default()},"cost":Cost::default(),"recordSource":"session","revision":1,"deleted":false,"statusCode":null,"legacyDate":null,"legacyTimezone":null,"timing":UsageTiming::estimate(1000,5000+index*1000,Some(2000),"codex_token_count","codex_item_started_at_ms")})).unwrap();
        s.transaction(|| s.upsert(r, "fixture")).unwrap();
        drop(s);
        clients.push(e);
    }
    let run = clients.clone();
    tokio::task::spawn_blocking(move || {
        run[0].sync().unwrap();
        run[1].sync().unwrap();
        run[0].sync().unwrap();
    })
    .await
    .unwrap();
    for e in &clients {
        assert_eq!(e.lock().unwrap().timings().unwrap().len(), 2);
        assert_eq!(e.lock().unwrap().records("all").unwrap().len(), 2);
        assert_eq!(e.lock().unwrap().pending_count().unwrap(), 0);
    }
    {
        let s = clients[0].lock().unwrap();
        let mut r = s.local_records().unwrap().remove(0);
        r.timing.as_mut().unwrap().duration_ms = 6000;
        s.transaction(|| s.upsert(r, "fixture")).unwrap();
    }
    let run = clients.clone();
    tokio::task::spawn_blocking(move || {
        run[0].sync().unwrap();
        run[1].sync().unwrap();
    })
    .await
    .unwrap();
    let device = clients[0].lock().unwrap().device_id().unwrap();
    assert_eq!(
        clients[1].lock().unwrap().timings().unwrap()[&(device.clone(), "same-native-id".into())]
            .duration_ms,
        6000
    );
    {
        let s = server.lock().unwrap();
        s.conn
            .execute_batch(
                "DELETE FROM server_records; DELETE FROM server_changes; DELETE FROM devices;",
            )
            .unwrap();
        s.set_meta("server_instance_id", &"replacement-v4").unwrap();
    }
    let run = clients.clone();
    tokio::task::spawn_blocking(move || {
        run[0].sync().unwrap();
    })
    .await
    .unwrap();
    assert_eq!(clients[0].lock().unwrap().timings().unwrap().len(), 1);
    assert_eq!(
        clients[0].lock().unwrap().timings().unwrap()[&(device, "same-native-id".into())]
            .duration_ms,
        6000
    );
    let run = clients.clone();
    tokio::task::spawn_blocking(move || {
        run[1].sync().unwrap();
        run[0].sync().unwrap();
    })
    .await
    .unwrap();
    for e in &clients {
        assert_eq!(e.lock().unwrap().timings().unwrap().len(), 2);
        assert_eq!(e.lock().unwrap().records("all").unwrap().len(), 2);
    }
    handle.abort();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_devices_sync_retry_and_server_recreation() {
    let dir = tempfile::tempdir().unwrap();
    let server = Arc::new(Engine::open(&dir.path().join("server.db")).unwrap());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = server_router(server.clone(), TOKEN.into()).unwrap();
    let handle = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut clients = Vec::new();
    for (name, harness, fixture) in [
        (
            "a",
            "claude",
            include_str!("../../tests/fixtures/claude/session.jsonl"),
        ),
        (
            "b",
            "codex",
            include_str!("../../tests/fixtures/codex/session.jsonl"),
        ),
    ] {
        let root = dir.path().join(name);
        fs::create_dir(&root).unwrap();
        fs::write(
            root.join("fixture.jsonl"),
            if harness == "codex" {
                fixture.replace("\"cwd\":", "\"model_provider\":\"openai\",\"cwd\":")
            } else {
                fixture.into()
            },
        )
        .unwrap();
        let e = Arc::new(Engine::open(&dir.path().join(format!("{name}.db"))).unwrap());
        let s = e.lock().unwrap();
        let mut c = s.config().unwrap();
        c.sources = vec![Source {
            id: name.into(),
            harness: harness.into(),
            root: root.to_string_lossy().into(),
            enabled: true,
        }];
        c.server_url = format!("http://{address}");
        c.server_token = TOKEN.into();
        c.device_name = name.into();
        s.set_meta("config", &c).unwrap();
        drop(s);
        clients.push(e);
    }
    let clients2 = clients.clone();
    tokio::task::spawn_blocking(move || {
        for e in &clients2 {
            e.scan(false).unwrap();
            e.sync().unwrap();
        }
        clients2[0].sync().unwrap();
        clients2[0].sync().unwrap();
    })
    .await
    .unwrap();
    assert_eq!(clients[0].lock().unwrap().records("all").unwrap().len(), 3);
    assert_eq!(clients[0].lock().unwrap().pending_count().unwrap(), 0);
    assert!(clients[0]
        .lock()
        .unwrap()
        .records("all")
        .unwrap()
        .iter()
        .filter(|r| r.harness == "codex")
        .all(|r| r.provider.as_deref() == Some("openai")));
    assert_eq!(
        harness_scope_core::sync::changes(&server.lock().unwrap(), 0, 500)
            .unwrap()
            .changes
            .len(),
        3
    );
    // Same endpoint, new instance: the client must rebuild its mirror and republish local facts.
    {
        let s = server.lock().unwrap();
        s.conn
            .execute_batch(
                "DELETE FROM server_records; DELETE FROM server_changes; DELETE FROM devices;",
            )
            .unwrap();
        s.set_meta("server_instance_id", &"replacement-server")
            .unwrap();
    }
    let clients2 = clients.clone();
    tokio::task::spawn_blocking(move || {
        clients2[0].sync().unwrap();
        clients2[1].sync().unwrap();
        clients2[0].sync().unwrap();
    })
    .await
    .unwrap();
    assert_eq!(clients[0].lock().unwrap().records("all").unwrap().len(), 3);
    assert_eq!(
        harness_scope_core::sync::changes(&server.lock().unwrap(), 0, 500)
            .unwrap()
            .changes
            .len(),
        3
    );
    let summary = clients[0]
        .execute("usage_query", json!({"kind":"summary","scope":"all"}))
        .unwrap();
    assert_eq!(summary["realTotalTokens"], 410);
    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_old_server_is_rejected_without_acknowledging_pending_records() {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = axum::Router::new().route(
        "/health",
        axum::routing::get(|| async { axum::Json(json!({"status":"ok","protocolVersion":3})) }),
    );
    let handle = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let root = dir.path().join("logs");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("fixture.jsonl"),
        include_str!("../../tests/fixtures/claude/session.jsonl"),
    )
    .unwrap();
    let e = Arc::new(Engine::open(&dir.path().join("client.db")).unwrap());
    {
        let s = e.lock().unwrap();
        let mut c = s.config().unwrap();
        c.sources = vec![Source {
            id: "fixture".into(),
            harness: "claude".into(),
            root: root.to_string_lossy().into(),
            enabled: true,
        }];
        c.server_url = format!("http://{address}");
        c.server_token = TOKEN.into();
        s.set_meta("config", &c).unwrap();
    }
    let client = e.clone();
    let error = tokio::task::spawn_blocking(move || {
        client.scan(false).unwrap();
        client.sync().unwrap_err().to_string()
    })
    .await
    .unwrap();
    assert!(error.contains("upgraded to protocol v4"));
    assert_eq!(e.lock().unwrap().pending_count().unwrap(), 1);
    assert_eq!(e.lock().unwrap().local_records().unwrap().len(), 1);
    handle.abort();
}
