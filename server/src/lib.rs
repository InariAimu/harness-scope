use axum::{
    extract::{DefaultBodyLimit, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use harness_scope_core::accounts::sharing;
use harness_scope_core::{model::PushRequest, sync, Engine};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tower_http::{compression::CompressionLayer, decompression::RequestDecompressionLayer};

#[derive(Clone)]
struct ServerState {
    engine: Arc<Engine>,
    token: Arc<String>,
}
type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;
fn error(e: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error":e.to_string()})),
    )
}
fn auth(headers: &HeaderMap, token: &str) -> Result<(), (StatusCode, Json<Value>)> {
    let actual = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if actual == format!("Bearer {token}") {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"Unauthorized"})),
        ))
    }
}
pub fn server_router(engine: Arc<Engine>, token: String) -> anyhow::Result<Router> {
    anyhow::ensure!(
        token.len() >= 24,
        "Set a server token of at least 24 characters"
    );
    let schema_version = engine.lock()?.schema_version()?;
    Ok(Router::new()
        .route(
            "/health",
            get(move || async move {
                Json(json!({"status":"ok","appBuild":harness_scope_core::build_info::current(),"schemaVersion":schema_version,"protocolVersion":4,"supportedProtocolVersions":[1,2,3,4],"requestEncodings":["identity","gzip"],"recordCapabilities":["claude_request_timing_v2",sharing::CAPABILITY]}))
            }),
        )
        .route("/v1/push", post(push))
        .route("/v1/changes", get(changes))
        .route("/v1/subscriptions/push", post(push_subscriptions))
        .route("/v1/subscriptions/changes", get(subscription_changes))
        .layer(DefaultBodyLimit::max(5 * 1024 * 1024))
        .layer(RequestDecompressionLayer::new().gzip(true))
        .layer(CompressionLayer::new().gzip(true))
        .with_state(ServerState {
            engine,
            token: Arc::new(token),
        }))
}
async fn push_subscriptions(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Json(request): Json<sharing::Push>,
) -> ApiResult {
    auth(&headers, &state.token)?;
    tokio::task::spawn_blocking(move || {
        sharing::push(&*state.engine.lock()?, request)
            .and_then(|v| serde_json::to_value(v).map_err(Into::into))
    })
    .await
    .map_err(error)?
    .map(Json)
    .map_err(error)
}
async fn subscription_changes(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Query(query): Query<ChangeQuery>,
) -> ApiResult {
    auth(&headers, &state.token)?;
    tokio::task::spawn_blocking(move || {
        sharing::changes(&*state.engine.lock()?, query.after, query.limit)
            .and_then(|v| serde_json::to_value(v).map_err(Into::into))
    })
    .await
    .map_err(error)?
    .map(Json)
    .map_err(error)
}
async fn push(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Json(request): Json<PushRequest>,
) -> ApiResult {
    auth(&headers, &state.token)?;
    tokio::task::spawn_blocking(move || {
        let store = state.engine.lock()?;
        sync::push(&store, request).and_then(|r| serde_json::to_value(r).map_err(Into::into))
    })
    .await
    .map_err(error)?
    .map(Json)
    .map_err(error)
}
#[derive(Deserialize)]
struct ChangeQuery {
    #[serde(default = "default_protocol", rename = "protocolVersion")]
    protocol_version: u32,
    #[serde(default)]
    after: i64,
    #[serde(default = "default_limit")]
    limit: usize,
}
fn default_protocol() -> u32 {
    1
}
fn default_limit() -> usize {
    500
}
async fn changes(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Query(query): Query<ChangeQuery>,
) -> ApiResult {
    auth(&headers, &state.token)?;
    tokio::task::spawn_blocking(move || {
        let store = state.engine.lock()?;
        let response = sync::changes(&store, query.after, query.limit)?;
        anyhow::ensure!(
            [1, 2, 3, 4].contains(&query.protocol_version),
            "Unsupported protocolVersion"
        );
        anyhow::ensure!(
            query.protocol_version >= 3
                || response
                    .changes
                    .iter()
                    .all(|c| c.record.request_metadata.is_none()),
            "Upgrade client to protocol v3 to read request metadata"
        );
        anyhow::ensure!(
            query.protocol_version >= 4
                || response.changes.iter().all(|c| c.record.timing.is_none()),
            "Upgrade client to protocol v4 to read timing metadata"
        );
        serde_json::to_value(response).map_err(Into::into)
    })
    .await
    .map_err(error)?
    .map(Json)
    .map_err(error)
}
#[derive(Deserialize)]
struct CommandRequest {
    name: String,
    #[serde(default)]
    args: Value,
}
pub fn local_router(engine: Arc<Engine>) -> Router {
    Router::new()
        .route(
            "/api/health",
            get(|| async { Json(json!({"status":"ok"})) }),
        )
        .route("/api/command", post(command))
        .layer(DefaultBodyLimit::max(5 * 1024 * 1024))
        .with_state(engine)
}
async fn command(
    State(engine): State<Arc<Engine>>,
    headers: HeaderMap,
    Json(request): Json<CommandRequest>,
) -> ApiResult {
    if let Some(origin) = headers.get("origin").and_then(|h| h.to_str().ok()) {
        if !["http://127.0.0.1:1420", "http://localhost:1420"].contains(&origin) {
            return Err((
                StatusCode::FORBIDDEN,
                Json(json!({"error":"Untrusted local API origin"})),
            ));
        }
    }
    tokio::task::spawn_blocking(move || engine.execute(&request.name, request.args))
        .await
        .map_err(error)?
        .map(Json)
        .map_err(error)
}
