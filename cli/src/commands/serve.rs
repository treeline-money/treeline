//! Serve command - expose Treeline as an HTTP hub
//!
//! Starts an HTTP server that serves two audiences:
//! 1. Full peers - push/pull sync bundles
//! 2. Thin clients - MCP over HTTP (Streamable HTTP transport)

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Json;
use serde_json::json;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};

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
    let hub_service = HubService::new(treeline_dir.clone(), "treeline.duckdb".to_string());

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

    // CORS layer for MCP clients (ChatGPT, browser-based clients)
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
        .allow_headers(Any)
        .expose_headers([axum::http::header::HeaderName::from_static(
            "mcp-session-id",
        )]);

    // Build and run the server
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let app = axum::Router::new()
            .route("/health", get(health))
            .route(
                "/api/push",
                post(handle_push).layer(DefaultBodyLimit::max(500 * 1024 * 1024)),
            )
            .route("/api/pull", get(handle_pull))
            .route("/mcp", post(handle_mcp).get(handle_mcp_get).delete(handle_mcp_delete))
            .layer(cors)
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
            Json(json!({ "error": e.to_string() })),
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
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

// ============================================================================
// MCP over HTTP (Streamable HTTP transport)
// ============================================================================

/// POST /mcp — main MCP protocol endpoint
async fn handle_mcp(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if let Err(e) = check_auth(&state.treeline_dir, &headers) {
        return e;
    }

    // Check that a database exists before trying to handle MCP requests
    if !state.hub_service.has_database() {
        let resp = mcp::JsonRpcResponse::error(
            serde_json::Value::Null,
            -32000,
            "No database on hub yet. Push a database first.".to_string(),
        );
        return (StatusCode::BAD_REQUEST, Json(resp)).into_response();
    }

    // Parse the JSON-RPC request
    let req: mcp::JsonRpcRequest = match serde_json::from_slice(&body) {
        Ok(req) => req,
        Err(e) => {
            let resp = mcp::JsonRpcResponse::error(
                serde_json::Value::Null,
                -32700,
                format!("Parse error: {}", e),
            );
            return (StatusCode::BAD_REQUEST, Json(resp)).into_response();
        }
    };

    // Determine if this is a write operation
    let is_write = if req.method == "tools/call" {
        req.params
            .as_ref()
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .map(|name| {
                matches!(name, "query_write" | "sync" | "tag" | "demo" | "skills_write")
            })
            .unwrap_or(false)
    } else {
        false
    };

    // Acquire appropriate lock
    let response = if is_write {
        let _lock = state.db_lock.write().await;
        mcp::handle_request(&req)
    } else {
        let _lock = state.db_lock.read().await;
        mcp::handle_request(&req)
    };

    match response {
        Some(resp) => (StatusCode::OK, Json(resp)).into_response(),
        None => StatusCode::ACCEPTED.into_response(), // Notification — no response
    }
}

/// GET /mcp — SSE endpoint (not implemented yet, returns 405)
async fn handle_mcp_get() -> impl IntoResponse {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(json!({ "error": "SSE transport not supported. Use POST." })),
    )
}

/// DELETE /mcp — session cleanup (no-op for stateless server)
async fn handle_mcp_delete() -> impl IntoResponse {
    StatusCode::OK
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
