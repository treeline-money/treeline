//! Serve command - expose Treeline as an HTTP hub
//!
//! Starts an HTTP server that serves two audiences:
//! 1. Full peers - push/pull database files for sync
//! 2. Thin clients - JSON-RPC API for querying/mutating data

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Json;
use serde_json::{json, Value};
use tokio::sync::RwLock;

use treeline_core::services::hub::HubService;

use super::get_treeline_dir;
use super::mcp;

/// Shared state for the HTTP server
struct AppState {
    hub_service: HubService,
    treeline_dir: std::path::PathBuf,
    /// RwLock to allow concurrent reads (pulls, queries) but exclusive writes (pushes)
    db_lock: RwLock<()>,
}

pub fn run(host: &str, port: u16) -> Result<()> {
    let treeline_dir = get_treeline_dir();

    // Ensure directory exists
    std::fs::create_dir_all(&treeline_dir)?;

    // Load or create auth token
    let token = HubService::load_or_create_token(&treeline_dir)?;

    // Create hub service — no database required at startup.
    // The database arrives via the first push.
    let hub_service = HubService::new(
        treeline_dir.clone(),
        "treeline.duckdb".to_string(),
    );

    let has_db = hub_service.has_database();

    let state = Arc::new(AppState {
        hub_service,
        treeline_dir: treeline_dir.clone(),
        db_lock: RwLock::new(()),
    });

    // Print server info
    let addr: SocketAddr = format!("{}:{}", host, port).parse()?;
    eprintln!("Treeline hub starting on http://{}", addr);
    eprintln!("Auth token: {}", token);
    if !has_db {
        eprintln!();
        eprintln!("No database yet. Waiting for first push.");
    }
    eprintln!();
    eprintln!("Link a client with:");
    eprintln!("  tl hub link http://{} --token {}", addr, token);

    // Build and run the server
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let app = axum::Router::new()
            .route("/health", get(health))
            .route("/api/push", post(handle_push).layer(DefaultBodyLimit::max(500 * 1024 * 1024))) // 500MB
            .route("/api/pull", get(handle_pull))
            .route("/api/tools", get(handle_tools_list))
            .route("/api/tools/call", post(handle_tools_call))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;
        Ok(())
    })
}

// ============================================================================
// Handlers
// ============================================================================

async fn health() -> &'static str {
    "ok"
}

async fn handle_push(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if let Err(e) = check_auth(&state.treeline_dir, &headers) {
        return e;
    }

    // Acquire write lock
    let _lock = state.db_lock.write().await;

    match state.hub_service.accept_push(&body) {
        Ok(result) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "backup": result.backup_name,
                "bytes_received": result.bytes_received,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": e.to_string(),
            })),
        )
            .into_response(),
    }
}

async fn handle_pull(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = check_auth(&state.treeline_dir, &headers) {
        return e;
    }

    // Acquire read lock
    let _lock = state.db_lock.read().await;

    match state.hub_service.get_bundle_for_pull() {
        Ok(bytes) => (
            StatusCode::OK,
            [(
                axum::http::header::CONTENT_TYPE,
                "application/octet-stream",
            )],
            bytes,
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": e.to_string(),
            })),
        )
            .into_response(),
    }
}

// ============================================================================
// Thin Client API
// ============================================================================

async fn handle_tools_list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = check_auth(&state.treeline_dir, &headers) {
        return e;
    }

    let tools = mcp::tool_definitions();
    (StatusCode::OK, Json(tools)).into_response()
}

#[derive(serde::Deserialize)]
struct ToolCallRequest {
    name: String,
    #[serde(default)]
    arguments: Value,
}

async fn handle_tools_call(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<ToolCallRequest>,
) -> impl IntoResponse {
    if let Err(e) = check_auth(&state.treeline_dir, &headers) {
        return e;
    }

    // Check that a database exists before trying to run tools
    if !state.hub_service.has_database() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "No database on hub yet. Push a database first."})),
        )
            .into_response();
    }

    // Acquire read lock for queries, write lock for mutations
    let is_write = matches!(
        req.name.as_str(),
        "query_write" | "sync" | "tag" | "demo" | "skills_write"
    );

    if is_write {
        let _lock = state.db_lock.write().await;
        execute_and_respond(&req.name, &req.arguments)
    } else {
        let _lock = state.db_lock.read().await;
        execute_and_respond(&req.name, &req.arguments)
    }
}

fn execute_and_respond(name: &str, arguments: &Value) -> axum::response::Response {
    match mcp::execute_tool(name, arguments) {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": err,
            })),
        )
            .into_response(),
    }
}

// ============================================================================
// Auth
// ============================================================================

fn check_auth(
    treeline_dir: &std::path::Path,
    headers: &HeaderMap,
) -> std::result::Result<(), axum::response::Response> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match token {
        None => Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Missing Authorization header"})),
        )
            .into_response()),
        Some(token) => match HubService::validate_token(treeline_dir, token) {
            Ok(true) => Ok(()),
            _ => Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Invalid token"})),
            )
                .into_response()),
        },
    }
}
