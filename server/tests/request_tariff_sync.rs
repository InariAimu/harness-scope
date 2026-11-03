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
const TOKEN: &str = "synthetic-tariff-sync-token-at-least-24";

#[test]
fn sync_server_open_preserves_metadata_without_adding_desktop_sources() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("server.db");
    let store = harness_scope_core::store::Store::open(&path).unwrap();
    let before = store
        .conn
        .prepare("SELECT key,value FROM meta ORDER BY key")
        .unwrap()
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    drop(store);
    let engine = Engine::open_sync_server(&path).unwrap();
    let store = engine.lock().unwrap();
    let after = store
        .conn
        .prepare("SELECT key,value FROM meta ORDER BY key")
        .unwrap()
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(before, after);
    assert!(store.config().unwrap().sources.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tariffs_wait_for_server_capability_and_roundtrip_without_stripping() {
    let dir = tempfile::tempdir().unwrap();
    let server = Arc::new(Engine::open(&dir.path().join("server.db")).unwrap());
    let capable = Arc::new(AtomicBool::new(false));
    let flag = capable.clone();
    let router = axum::Router::new().route("/health", axum::routing::get(move || {
        let flag = flag.clone();
        async move { axum::Json(json!({"schemaVersion":7,"protocolVersion":4,"recordCapabilities": if flag.load(Ordering::SeqCst) {vec!["claude_request_timing_v2","request_tariff_v1"]} else {vec!["claude_request_timing_v2"]}})) }
    })).fallback_service(server_router(server.clone(), TOKEN.into()).unwrap());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
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
        let mut c = s.config().unwrap();
        c.sources = vec![Source {
            id: "fixture".into(),
            harness: "claude".into(),
            root: root.to_string_lossy().into(),
            enabled: true,
        }];
        c.server_url = url.clone();
        c.server_token = TOKEN.into();
        s.set_meta("config", &c).unwrap();
    }
    let run = client.clone();
    let expected=tokio::task::spawn_blocking(move || {
        run.scan(false).unwrap();
        let s=run.lock().unwrap();let mut r=s.records("local").unwrap().remove(0);
        let p=serde_json::from_value(json!({"modelId":r.model,"displayName":"synthetic tariff","provider":"custom","inputCostPerMillion":"8","outputCostPerMillion":"40","cacheReadCostPerMillion":"2","cacheCreationCostPerMillion":"10","source":"manual","sourceKey":null,"fetchedAt":0,"requestTariff":{"version":"test-v1","selectedTier":"fast","tiers":[{"id":"fast","speed":"fast","abovePromptTokens":0,"inputCostPerMillion":"8","outputCostPerMillion":"40","cacheReadCostPerMillion":"2","cacheCreationCostPerMillion":"10"}]}})).unwrap();
        r.cost.snapshot=Some(p);r.cost.amount_usd=Some("1.25".into());r.cost.rule_version="request-tariff-v1:test-v1:fast".into();
        s.upsert(r.clone(),"synthetic").unwrap();drop(s);
        let before=run.lock().unwrap().record(&r.id).unwrap().unwrap();
        for _ in 0..2 {
            let report=run.sync().unwrap();assert_eq!(report["deferredTariffs"],1);assert!(report["warning"].as_str().unwrap().contains("request_tariff_v1"));
            let s=run.lock().unwrap();assert_eq!(s.pending_count().unwrap(),1);assert_eq!(s.pending().unwrap()[0],before);
        }
        before
    }).await.unwrap();
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
        let result = run.sync().unwrap();
        assert_eq!(result["uploaded"], 1);
        assert_eq!(result["deferredTariffs"], 0);
        assert!(result["warning"].is_null());
        assert_eq!(run.lock().unwrap().pending_count().unwrap(), 0);
        assert_eq!(run.sync().unwrap()["uploaded"], 0);
    })
    .await
    .unwrap();
    let http = reqwest::Client::new();
    let old = http
        .get(format!(
            "{url}/v1/changes?after=0&limit=500&protocolVersion=4"
        ))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(old.status(), reqwest::StatusCode::BAD_REQUEST);
    assert!(old.text().await.unwrap().contains("request_tariff_v1"));
    let response=http.get(format!("{url}/v1/changes?after=0&limit=500&protocolVersion=4&recordCapabilities=request_tariff_v1")).bearer_auth(TOKEN).send().await.unwrap();
    assert!(response.status().is_success());
    let changes: harness_scope_core::model::ChangesResponse = response.json().await.unwrap();
    assert_eq!(
        changes
            .changes
            .iter()
            .find(|c| c.record.id == expected.id)
            .unwrap()
            .record,
        expected
    );
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
        let records = mirror.lock().unwrap().records("all").unwrap();
        assert_eq!(
            records.iter().find(|r| r.id == expected.id).unwrap(),
            &expected
        );
    })
    .await
    .unwrap();
    handle.abort();
}
