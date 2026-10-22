use axum::{
    extract::Request,
    http::StatusCode,
    middleware::{self, Next},
    response::IntoResponse,
};
use harness_scope_core::{model::Source, Engine};
use harness_scope_service::server_router;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

const TOKEN: &str = "synthetic-device-name-test-token";

async fn sync(client: &Arc<Engine>) -> anyhow::Result<Value> {
    let client = client.clone();
    tokio::task::spawn_blocking(move || client.sync())
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn names_sync_without_new_usage_and_survive_failures_restarts_and_server_recreation() {
    let dir = tempfile::tempdir().unwrap();
    let server = Arc::new(Engine::open(&dir.path().join("server.db")).unwrap());
    let fail = Arc::new(AtomicBool::new(false));
    let pushes = Arc::new(AtomicUsize::new(0));
    let fail_route = fail.clone();
    let pushes_route = pushes.clone();
    let router = server_router(server.clone(), TOKEN.into())
        .unwrap()
        .layer(middleware::from_fn(move |request: Request, next: Next| {
            let fail = fail_route.clone();
            let pushes = pushes_route.clone();
            async move {
                if request.uri().path() == "/v1/push" {
                    pushes.fetch_add(1, Ordering::SeqCst);
                    if fail.load(Ordering::SeqCst) {
                        return StatusCode::SERVICE_UNAVAILABLE.into_response();
                    }
                }
                next.run(request).await
            }
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let logs = dir.path().join("logs");
    std::fs::create_dir(&logs).unwrap();
    std::fs::write(
        logs.join("fixture.jsonl"),
        include_str!("../../tests/fixtures/claude/session.jsonl"),
    )
    .unwrap();
    let mut clients = Vec::new();
    for (index, name) in ["Windows", "macOS"].iter().enumerate() {
        let client =
            Arc::new(Engine::open(&dir.path().join(format!("client-{index}.db"))).unwrap());
        let store = client.lock().unwrap();
        let mut config = store.config().unwrap();
        config.device_name = name.to_string();
        config.server_url = format!("http://{address}");
        config.server_token = TOKEN.into();
        config.sources = vec![Source {
            id: "fixture".into(),
            harness: "claude".into(),
            root: logs.to_string_lossy().into(),
            enabled: index == 0,
        }];
        store.set_meta("config", &config).unwrap();
        drop(store);
        clients.push(client);
    }
    let scan = clients[0].clone();
    tokio::task::spawn_blocking(move || scan.scan(false).unwrap())
        .await
        .unwrap();
    sync(&clients[0]).await.unwrap();
    assert_eq!(sync(&clients[1]).await.unwrap()["uploaded"], 0);
    assert_eq!(clients[0].lock().unwrap().pending_count().unwrap(), 0);
    assert!(clients[1]
        .lock()
        .unwrap()
        .local_records()
        .unwrap()
        .is_empty());
    let original_id = clients[0].lock().unwrap().device_id().unwrap();
    let before = harness_scope_core::sync::changes(&server.lock().unwrap(), 0, 500).unwrap();
    assert_eq!(before.devices.len(), 2);
    assert!(!before.changes.is_empty());
    let count = pushes.load(Ordering::SeqCst);
    sync(&clients[0]).await.unwrap();
    sync(&clients[1]).await.unwrap();
    assert_eq!(
        pushes.load(Ordering::SeqCst),
        count,
        "unchanged names should not send empty pushes"
    );

    clients[0]
        .execute("rename_device", json!({"name":"  我的工作站  "}))
        .unwrap();
    fail.store(true, Ordering::SeqCst);
    assert!(sync(&clients[0]).await.is_err());
    assert_eq!(clients[0].lock().unwrap().pending_count().unwrap(), 0);
    fail.store(false, Ordering::SeqCst);
    clients[0] = Arc::new(Engine::open(&dir.path().join("client-0.db")).unwrap());
    assert_eq!(sync(&clients[0]).await.unwrap()["uploaded"], 0);
    sync(&clients[1]).await.unwrap();
    let snapshot = clients[1].execute("snapshot", json!({})).unwrap();
    assert!(snapshot["devices"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["id"] == original_id && d["name"] == "我的工作站"));
    assert_eq!(clients[0].lock().unwrap().device_id().unwrap(), original_id);
    let after = harness_scope_core::sync::changes(&server.lock().unwrap(), 0, 500).unwrap();
    assert_eq!(
        serde_json::to_value(before.changes).unwrap(),
        serde_json::to_value(after.changes).unwrap()
    );
    assert_eq!(before.next_cursor, after.next_cursor);

    // Same names remain separate devices, including a device with no local usage.
    clients[1]
        .execute("rename_device", json!({"name":"我的工作站"}))
        .unwrap();
    sync(&clients[1]).await.unwrap();
    sync(&clients[0]).await.unwrap();
    let devices = clients[0].execute("snapshot", json!({})).unwrap()["devices"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(devices.len(), 2);
    assert!(devices.iter().all(|d| d["name"] == "我的工作站"));
    assert_ne!(devices[0]["id"], devices[1]["id"]);
    {
        let store = server.lock().unwrap();
        store.conn.execute("DELETE FROM devices", []).unwrap();
        store
            .set_meta("server_instance_id", &"replacement-server")
            .unwrap();
    }
    assert_eq!(sync(&clients[1]).await.unwrap()["uploaded"], 0);
    let devices = harness_scope_core::sync::changes(&server.lock().unwrap(), 0, 500)
        .unwrap()
        .devices;
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0]["name"], "我的工作站");
    handle.abort();
}
