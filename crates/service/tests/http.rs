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
            include_str!("../../../tests/fixtures/claude/session.jsonl"),
        ),
        (
            "b",
            "codex",
            include_str!("../../../tests/fixtures/codex/session.jsonl"),
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
        axum::routing::get(|| async { axum::Json(json!({"status":"ok","protocolVersion":1})) }),
    );
    let handle = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let root = dir.path().join("logs");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("fixture.jsonl"),
        include_str!("../../../tests/fixtures/claude/session.jsonl"),
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
    assert!(error.contains("upgraded to protocol v2"));
    assert_eq!(e.lock().unwrap().pending_count().unwrap(), 1);
    assert_eq!(e.lock().unwrap().local_records().unwrap().len(), 1);
    handle.abort();
}
