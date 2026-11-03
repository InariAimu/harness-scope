use harness_scope_core::{model::Source, Engine};
use harness_scope_service::server_router;
use serde_json::json;
use std::{
    fs,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
const TOKEN: &str = "synthetic-model-usage-sync-token-24";
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn old_server_defers_models_but_other_records_sync_then_full_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let server = Arc::new(Engine::open_sync_server(&dir.path().join("server.db")).unwrap());
    let capable = Arc::new(AtomicBool::new(false));
    let flag = capable.clone();
    let router=axum::Router::new().route("/health",axum::routing::get(move||{let flag=flag.clone();async move{axum::Json(json!({"schemaVersion":8,"protocolVersion":4,"recordCapabilities":if flag.load(Ordering::SeqCst){vec!["claude_request_timing_v2","request_tariff_v1","model_usage_v2"]}else{vec!["claude_request_timing_v2","request_tariff_v1"]}}))}})).fallback_service(server_router(server.clone(),TOKEN.into()).unwrap());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let root = dir.path().join("logs");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("models.jsonl"),
        include_str!("../../tests/fixtures/gpt-img-studio/model-usage.jsonl"),
    )
    .unwrap();
    let client = Arc::new(Engine::open(&dir.path().join("client.db")).unwrap());
    {
        let s = client.lock().unwrap();
        let mut c = s.config().unwrap();
        c.sources = vec![Source {
            id: "fixture".into(),
            harness: "gpt-img-studio".into(),
            root: root.to_string_lossy().into(),
            enabled: true,
        }];
        c.server_url = url.clone();
        c.server_token = TOKEN.into();
        s.set_meta("config", &c).unwrap();
    }
    let run = client.clone();
    let expected = tokio::task::spawn_blocking(move || {
        run.scan(false).unwrap();
        let s = run.lock().unwrap();
        let original = s.local_records().unwrap();
        assert_eq!(original.len(), 2);
        let mut http = original[0].clone();
        http.id = "normal-http".into();
        http.model_usage = None;
        http.harness = "codex".into();
        http.count = 1;
        http.cost = Default::default();
        s.upsert(http, "synthetic").unwrap();
        drop(s);
        for _ in 0..2 {
            let result = run.sync().unwrap();
            assert_eq!(result["deferredModelUsage"], 2);
            assert!(result["warning"]
                .as_str()
                .unwrap()
                .contains("model_usage_v2"));
            assert_eq!(run.lock().unwrap().pending_count().unwrap(), 2);
        }
        original
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
        assert_eq!(run.sync().unwrap()["uploaded"], 2);
        assert_eq!(run.sync().unwrap()["uploaded"], 0);
        assert_eq!(run.lock().unwrap().pending_count().unwrap(), 0);
    })
    .await
    .unwrap();
    let http = reqwest::Client::new();
    let old=http.get(format!("{url}/v1/changes?after=0&limit=500&protocolVersion=4&recordCapabilities=request_tariff_v1,model_usage_v1")).bearer_auth(TOKEN).send().await.unwrap();
    assert_eq!(old.status(), reqwest::StatusCode::BAD_REQUEST);
    assert!(old.text().await.unwrap().contains("model_usage_v2"));
    let mirror = Arc::new(Engine::open(&dir.path().join("mirror.db")).unwrap());
    {
        let s = mirror.lock().unwrap();
        let mut c = s.config().unwrap();
        c.sources.clear();
        c.server_url = url;
        c.server_token = TOKEN.into();
        s.set_meta("config", &c).unwrap();
    }
    tokio::task::spawn_blocking(move || {
        mirror.sync().unwrap();
        let s = mirror.lock().unwrap();
        let received = s.records("all").unwrap();
        for record in expected {
            assert_eq!(
                received.iter().find(|r| r.id == record.id).unwrap(),
                &record
            );
        }
        assert_eq!(received.len(), 3);
    })
    .await
    .unwrap();
    handle.abort();
}
