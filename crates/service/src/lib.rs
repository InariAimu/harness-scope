use axum::{
    extract::{DefaultBodyLimit, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use harness_scope_core::{model::PushRequest, sync, Engine};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

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
    Ok(Router::new()
        .route(
            "/health",
            get(|| async {
                Json(json!({"status":"ok","protocolVersion":2,"supportedProtocolVersions":[1,2]}))
            }),
        )
        .route("/v1/push", post(push))
        .route("/v1/changes", get(changes))
        .layer(DefaultBodyLimit::max(5 * 1024 * 1024))
        .with_state(ServerState {
            engine,
            token: Arc::new(token),
        }))
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
    #[serde(default)]
    after: i64,
    #[serde(default = "default_limit")]
    limit: usize,
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
        sync::changes(&store, query.after, query.limit)
            .and_then(|r| serde_json::to_value(r).map_err(Into::into))
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
