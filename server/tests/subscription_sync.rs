use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json,
};
use harness_scope_core::{accounts::sharing, Engine};
use harness_scope_service::server_router;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

const TOKEN: &str = "subscription-test-token-not-a-real-secret";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn simultaneous_clients_keep_quota_samples_and_token_revisions_independent() {
    let dir = tempfile::tempdir().unwrap();
    let server = Arc::new(Engine::open(&dir.path().join("parallel-server.db")).unwrap());
    let reset = chrono::Utc::now().timestamp() + 86400;
    let router = axum::Router::new()
        .route("/quota", get(move || async move { Json(json!({"id":"weekly","usedPercent":20,"resetsAt":reset,"durationMinutes":10080,"windowType":"fixed"})) }))
        .fallback_service(server_router(server.clone(), TOKEN.into()).unwrap());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let path = dir.path().to_owned();
    let (a, b) = tokio::task::spawn_blocking(move || {
        let a = Arc::new(Engine::open(&path.join("parallel-a.db")).unwrap());
        let b = Arc::new(Engine::open(&path.join("parallel-b.db")).unwrap());
        configure(&a, &url); configure(&b, &url);
        let av = create(&a, &url, "new");
        a.sync().unwrap(); b.sync().unwrap();
        let bv = create(&b, &url, av["account"]["sharedSubscriptionId"].as_str().unwrap());
        for (e, view, input) in [(&a, av, 100), (&b, bv, 200)] {
            for _ in 0..3 {
                assert!(e.execute("provider_account_refresh", json!({"id":view["account"]["id"]})).unwrap()["error"].is_null());
            }
            let s = e.lock().unwrap();
            // Same event ID on two devices is two distinct usage facts.
            let row: harness_scope_core::model::UsageRecord = serde_json::from_value(json!({
                "deviceId":s.device_id().unwrap(),"id":"overlap","harness":"codex","provider":"openai",
                "parserVersion":"test","occurredAt":chrono::Utc::now().timestamp(),"model":"fixture",
                "grain":"event","count":1,"tokens":{"input":input,"output":0,"cacheRead":0,"cacheWrite":0,"reasoning":0},
                "cost":harness_scope_core::model::Cost::default(),"recordSource":"session","revision":1,"deleted":false
            })).unwrap();
            s.upsert(row, "synthetic").unwrap();
        }
        (a, b)
    }).await.unwrap();
    for round in 0..3 {
        let clients = [a.clone(), b.clone()];
        let jobs = clients.map(|e| {
            tokio::task::spawn_blocking(move || {
                if round == 1 {
                    let s = e.lock().unwrap();
                    let mut row = s.local_records().unwrap().remove(0);
                    row.tokens.input += 1;
                    s.upsert(row, "synthetic").unwrap();
                }
                e.sync().unwrap();
            })
        });
        for job in jobs {
            job.await.unwrap();
        }
    }
    tokio::task::spawn_blocking(move || {
        // Final pull after both pushes also covers whichever raced past the first pull.
        for e in [a, b] {
            e.sync().unwrap();
            let s = e.lock().unwrap();
            assert_eq!(sharing::pending_count(&s).unwrap(), 0);
            assert_eq!(
                s.conn
                    .query_row(
                        "SELECT COUNT(*) FROM provider_account_observations",
                        [],
                        |r| r.get::<_, u64>(0)
                    )
                    .unwrap(),
                6
            );
            assert!(s.pending().unwrap().is_empty());
            let facts: Vec<Value> = s
                .conn
                .prepare("SELECT payload FROM remote_records UNION ALL SELECT payload FROM records")
                .unwrap()
                .query_map([], |r| r.get::<_, String>(0))
                .unwrap()
                .map(|r| serde_json::from_str(&r.unwrap()).unwrap())
                .collect();
            assert_eq!(facts.len(), 2);
            assert_eq!(
                facts
                    .iter()
                    .map(|r| r["tokens"]["input"].as_u64().unwrap())
                    .sum::<u64>(),
                302
            );
        }
        let s = server.lock().unwrap();
        assert_eq!(
            s.conn
                .query_row("SELECT COUNT(*) FROM server_records", [], |r| r
                    .get::<_, u64>(0))
                .unwrap(),
            2
        );
        assert_eq!(sharing::changes(&s, 0, 500).unwrap().observations.len(), 6);
    })
    .await
    .unwrap();
    task.abort();
}

#[tokio::test]
async fn subscription_routes_require_authentication_and_validate_cursors() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let dir = tempfile::tempdir().unwrap();
    let e = Arc::new(Engine::open(&dir.path().join("auth.db")).unwrap());
    let router = server_router(e, TOKEN.into()).unwrap();
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/subscriptions/changes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response=router.clone().oneshot(Request::builder().method("POST").uri("/v1/subscriptions/push").header("Content-Type","application/json").body(Body::from(json!({"deviceId":"device","serverInstanceId":"instance","subscriptions":[],"observations":[]}).to_string())).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/subscriptions/changes?after=-1")
                .header("Authorization", format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

fn configure(e: &Engine, url: &str) {
    let s = e.lock().unwrap();
    let mut c = s.config().unwrap();
    c.server_url = url.into();
    c.server_token = TOKEN.into();
    c.sources.clear();
    s.set_meta("config", &c).unwrap();
}
fn create(e: &Engine, url: &str, shared: &str) -> Value {
    e.execute("provider_account_save",json!({"provider":"custom","name":"HTTP fixture","sharedSubscriptionId":shared,
        "refresh":{"foregroundSeconds":0,"backgroundSeconds":0},
        "custom":{"enabled":true,"baseUrl":url,"timeoutSeconds":5,"script":"({request:{url:'{{baseUrl}}/quota'},extractor(r){return r;}})"}})).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_engine_handoff_retries_paginates_and_recovers_without_losing_history() {
    let dir = tempfile::tempdir().unwrap();
    let server = Arc::new(Engine::open(&dir.path().join("server.db")).unwrap());
    let capable = Arc::new(AtomicBool::new(false));
    let lose_response = Arc::new(AtomicBool::new(false));
    let percent = Arc::new(AtomicU64::new(20));
    let reset = chrono::Utc::now().timestamp() + 86400;
    let cap = capable.clone();
    let lost = lose_response.clone();
    let srv = server.clone();
    let used = percent.clone();
    let router=axum::Router::new()
        .route("/health",get(move || { let cap=cap.clone(); async move { Json(json!({"protocolVersion":4,"schemaVersion":6,"recordCapabilities":if cap.load(Ordering::SeqCst){vec![sharing::CAPABILITY,"claude_request_timing_v2"]}else{vec!["claude_request_timing_v2"]}})) }}))
        .route("/quota",get(move || { let used=used.clone(); async move { Json(json!({"id":"weekly","usedPercent":used.load(Ordering::SeqCst),"resetsAt":reset,"durationMinutes":10080,"windowType":"fixed","planName":"Plan","unit":"USD","total":"100"})) }}))
        .route("/v1/subscriptions/push",post(move |State(()):State<()>, headers:HeaderMap, Json(body):Json<sharing::Push>| {
            let srv=srv.clone(); let lost=lost.clone(); async move {
                if headers.get("authorization").and_then(|v|v.to_str().ok()) != Some(&format!("Bearer {TOKEN}")) { return (StatusCode::UNAUTHORIZED,Json(json!({"error":"unauthorized"}))); }
                let value=tokio::task::spawn_blocking(move || sharing::push(&srv.lock().unwrap(),body)).await.unwrap();
                match value {
                    Ok(_) if lost.swap(false,Ordering::SeqCst) => (StatusCode::SERVICE_UNAVAILABLE,Json(json!({"error":"simulated lost acknowledgment"}))),
                    Ok(value) => (StatusCode::OK,Json(serde_json::to_value(value).unwrap())),
                    Err(e) => (StatusCode::BAD_REQUEST,Json(json!({"error":e.to_string()}))),
                }
            }
        }))
        .layer(tower_http::decompression::RequestDecompressionLayer::new().gzip(true))
        .fallback_service(server_router(server.clone(),TOKEN.into()).unwrap());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let path = dir.path().to_owned();
    tokio::task::spawn_blocking(move || {
        let a=Engine::open(&path.join("a.db")).unwrap(); configure(&a,&url);
        let av=create(&a,&url,"new"); let aid=av["account"]["id"].as_str().unwrap();
        let shared=av["account"]["sharedSubscriptionId"].as_str().unwrap();
        let refreshed=a.execute("provider_account_refresh",json!({"id":aid})).unwrap();
        assert!(refreshed["error"].is_null(),"{refreshed}");
        let legacy=a.sync().unwrap();
        assert!(legacy["warning"].as_str().unwrap().contains("subscription_history_v1"));
        assert_eq!(legacy["subscriptionPending"],2);
        capable.store(true,Ordering::SeqCst); lose_response.store(true,Ordering::SeqCst);
        assert!(a.sync().is_err()); // The server committed but the acknowledgment was lost.
        assert_eq!(sharing::pending_count(&a.lock().unwrap()).unwrap(),2);
        assert_eq!(sharing::changes(&server.lock().unwrap(),0,500).unwrap().observations.len(),1);
        a.sync().unwrap();
        assert_eq!(sharing::pending_count(&a.lock().unwrap()).unwrap(),0);
        // Simulate pre-upgrade persisted samples, including more than one upload/download page.
        {
            let s=a.lock().unwrap();
            let original:String=s.conn.query_row("SELECT payload FROM provider_account_observations LIMIT 1",[],|r|r.get(0)).unwrap();
            let original:Value=serde_json::from_str(&original).unwrap();
            for n in 1..=501 {
                let mut old=original.clone(); old["refreshId"]=json!(uuid::Uuid::new_v4().to_string());
                let at=original["observedAtMs"].as_i64().unwrap()-n*60000; old["observedAtMs"]=json!(at);
                s.conn.execute("INSERT INTO provider_account_observations(refresh_id,account_id,provider,observed_at_ms,payload) VALUES(?1,?2,?3,?4,?5)",rusqlite::params![old["refreshId"].as_str().unwrap(),aid,old["provider"].as_str().unwrap(),at,old.to_string()]).unwrap();
            }
        }
        let uploaded=a.sync().unwrap(); assert_eq!(uploaded["subscriptionSync"]["uploaded"],501);
        drop(a);
        let b=Engine::open(&path.join("b.db")).unwrap(); configure(&b,&url);
        let downloaded=b.sync().unwrap(); assert_eq!(downloaded["subscriptionSync"]["downloaded"],502);
        let bv=create(&b,&url,shared); let bid=bv["account"]["id"].as_str().unwrap();
        let page=b.execute("provider_account_history",json!({"id":bid,"limit":500})).unwrap();
        assert_eq!(page["observations"].as_array().unwrap().len(),500);
        let older=b.execute("provider_account_history",json!({"id":bid,"beforeSequence":page["nextCursor"],"limit":500})).unwrap();
        assert_eq!(older["observations"].as_array().unwrap().len(),2);
        percent.store(30,Ordering::SeqCst);
        assert!(b.execute("provider_account_refresh",json!({"id":bid})).unwrap()["error"].is_null());
        b.sync().unwrap();
        let a=Engine::open(&path.join("a.db")).unwrap();
        assert_eq!(a.sync().unwrap()["subscriptionSync"]["downloaded"],1);
        assert_eq!(a.sync().unwrap()["subscriptionSync"]["downloaded"],0);
        assert_eq!(sharing::pending_count(&a.lock().unwrap()).unwrap(),0);
        // Restore an empty server with a new instance ID, then let devices return sequentially.
        {
            let s=server.lock().unwrap();
            s.conn.execute("DELETE FROM server_subscription_observations",[]).unwrap();
            s.conn.execute("DELETE FROM shared_subscriptions",[]).unwrap();
            s.set_meta("server_instance_id",&uuid::Uuid::new_v4().to_string()).unwrap();
        }
        a.sync().unwrap(); b.sync().unwrap(); a.sync().unwrap();
        let s=server.lock().unwrap();
        assert_eq!(s.conn.query_row("SELECT COUNT(*) FROM server_subscription_observations",[],|r|r.get::<_,u64>(0)).unwrap(),503);
        let s=a.lock().unwrap();
        assert_eq!(s.conn.query_row("SELECT COUNT(*) FROM provider_account_observations",[],|r|r.get::<_,u64>(0)).unwrap(),503);
        assert_eq!(sharing::pending_count(&s).unwrap(),0);
    }).await.unwrap();
    task.abort();
}
