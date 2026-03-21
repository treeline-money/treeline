//! Serve command - expose Treeline as an HTTP hub
//!
//! Starts an HTTP server that serves two audiences:
//! 1. Full peers - push/pull sync bundles
//! 2. Thin clients - MCP over HTTP (Streamable HTTP transport)
//!
//! Includes OAuth 2.1 endpoints for MCP client authentication.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{Html, IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::Json;
use serde_json::json;
use tokio::sync::{Mutex, RwLock};
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
    /// OAuth state: pending auth codes (code -> redirect_uri)
    /// Codes expire after use (single-use).
    auth_codes: Mutex<HashMap<String, AuthCodeEntry>>,
    /// OAuth state: registered clients (client_id -> client info)
    oauth_clients: Mutex<HashMap<String, OAuthClient>>,
}

struct AuthCodeEntry {
    redirect_uri: String,
    code_challenge: Option<String>,
    _created_at: std::time::Instant,
}

#[allow(dead_code)]
struct OAuthClient {
    client_id: String,
    redirect_uris: Vec<String>,
}

pub fn run(host: &str, port: u16) -> Result<()> {
    let treeline_dir = get_treeline_dir();
    std::fs::create_dir_all(&treeline_dir)?;

    let token = HubService::load_or_create_token(&treeline_dir)?;
    let hub_service = HubService::new(treeline_dir.clone(), "treeline.duckdb".to_string());
    let has_db = hub_service.has_database();

    let addr: SocketAddr = format!("{}:{}", host, port).parse()?;

    let state = Arc::new(AppState {
        hub_service,
        treeline_dir: treeline_dir.clone(),
        db_lock: RwLock::new(()),
        auth_codes: Mutex::new(HashMap::new()),
        oauth_clients: Mutex::new(HashMap::new()),
    });

    eprintln!("Treeline hub starting on http://{}", addr);
    eprintln!("Auth token: {}", token);
    if !has_db {
        eprintln!();
        eprintln!("No database yet. Waiting for first push.");
    }
    eprintln!();
    eprintln!("Link a client with:");
    eprintln!("  tl hub link http://{} --token {}", addr, token);

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
        .allow_headers(Any)
        .expose_headers([axum::http::header::HeaderName::from_static(
            "mcp-session-id",
        )]);

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let app = axum::Router::new()
            // Core endpoints
            .route("/health", get(health))
            .route(
                "/api/push",
                post(handle_push).layer(DefaultBodyLimit::max(500 * 1024 * 1024)),
            )
            .route("/api/pull", get(handle_pull))
            .route("/api/hash", get(handle_hash))
            .route(
                "/mcp",
                post(handle_mcp)
                    .get(handle_mcp_get)
                    .delete(handle_mcp_delete),
            )
            // OAuth 2.1 endpoints
            .route(
                "/.well-known/oauth-protected-resource",
                get(handle_oauth_protected_resource),
            )
            .route(
                "/.well-known/oauth-protected-resource/mcp",
                get(handle_oauth_protected_resource),
            )
            .route(
                "/.well-known/oauth-authorization-server",
                get(handle_oauth_metadata),
            )
            .route("/register", post(handle_oauth_register))
            .route("/authorize", get(handle_oauth_authorize))
            .route("/authorize", post(handle_oauth_authorize_submit))
            .route("/token", post(handle_oauth_token))
            .layer(cors)
            .with_state(state);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;
        Ok(())
    })
}

// ============================================================================
// Core Handlers
// ============================================================================

async fn health() -> &'static str {
    "ok"
}

#[derive(serde::Deserialize, Default)]
struct PushParams {
    base_hash: Option<String>,
}

async fn handle_push(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    params: Query<PushParams>,
    body: Bytes,
) -> impl IntoResponse {
    if let Err(e) = check_auth(&state.treeline_dir, &headers) {
        return e;
    }

    let base_hash = params.base_hash.clone();

    let _lock = state.db_lock.write().await;

    match state.hub_service.accept_push(&body, base_hash.as_deref()) {
        Ok(treeline_core::services::PushOutcome::Accepted { backup_name, bytes_received, new_hash }) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "backup": backup_name,
                "bytes_received": bytes_received,
                "hash": new_hash,
            })),
        )
            .into_response(),
        Ok(treeline_core::services::PushOutcome::Conflict { hub_hash }) => (
            StatusCode::CONFLICT,
            Json(json!({
                "status": "conflict",
                "hub_hash": hub_hash,
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

async fn handle_hash(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = check_auth(&state.treeline_dir, &headers) {
        return e;
    }

    let hash = state.hub_service.current_hash().unwrap_or(None);
    (StatusCode::OK, Json(json!({ "hash": hash }))).into_response()
}

// ============================================================================
// MCP over HTTP
// ============================================================================

async fn handle_mcp(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if let Err(e) = check_auth(&state.treeline_dir, &headers) {
        return e;
    }

    if !state.hub_service.has_database() {
        let resp = mcp::JsonRpcResponse::error(
            serde_json::Value::Null,
            -32000,
            "No database on hub yet. Push a database first.".to_string(),
        );
        return (StatusCode::BAD_REQUEST, Json(resp)).into_response();
    }

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

    let response = if is_write {
        let _lock = state.db_lock.write().await;
        let resp = mcp::handle_request(&req);
        // Recompute hash after writes so conflict detection stays current
        let _ = state.hub_service.compute_and_store_hash();
        resp
    } else {
        let _lock = state.db_lock.read().await;
        mcp::handle_request(&req)
    };

    match response {
        Some(resp) => (StatusCode::OK, Json(resp)).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

async fn handle_mcp_get(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    eprintln!("[mcp GET] Accept={:?} Auth={:?}",
        headers.get("accept"), headers.get("authorization").map(|_| "present"));
    if let Err(e) = check_auth(&state.treeline_dir, &headers) {
        return e;
    }
    // Acknowledge the connection. Full SSE streaming not implemented yet.
    StatusCode::OK.into_response()
}

async fn handle_mcp_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    eprintln!("[mcp DELETE]");
    if let Err(e) = check_auth(&state.treeline_dir, &headers) {
        return e;
    }
    StatusCode::OK.into_response()
}

// ============================================================================
// OAuth 2.1 — Self-hosted flow
//
// The hub is its own OAuth authorization server. The "authorization"
// is the user proving they possess the hub token.
// ============================================================================

/// GET /.well-known/oauth-protected-resource — RFC 9728
/// Tells the client where the authorization server is.
async fn handle_oauth_protected_resource(headers: HeaderMap) -> impl IntoResponse {
    let base = base_url_from_headers(&headers);
    eprintln!("[oauth/protected-resource] base_url={}", base);
    Json(json!({
        "resource": format!("{}/mcp", base),
        "authorization_servers": [base],
    }))
}

/// GET /.well-known/oauth-authorization-server
async fn handle_oauth_metadata(headers: HeaderMap) -> impl IntoResponse {
    let base = base_url_from_headers(&headers);
    eprintln!("[oauth/metadata] base_url={}", base);
    Json(json!({
        "issuer": base,
        "authorization_endpoint": format!("{}/authorize", base),
        "token_endpoint": format!("{}/token", base),
        "registration_endpoint": format!("{}/register", base),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code"],
        "token_endpoint_auth_methods_supported": ["none"],
        "code_challenge_methods_supported": ["S256"],
    }))
}

/// POST /register — Dynamic client registration (RFC 7591)
async fn handle_oauth_register(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    eprintln!("[oauth/register] body={}", body);
    let redirect_uris = body
        .get("redirect_uris")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let client_id = generate_random_id();

    let mut clients = state.oauth_clients.lock().await;
    clients.insert(
        client_id.clone(),
        OAuthClient {
            client_id: client_id.clone(),
            redirect_uris: redirect_uris.clone(),
        },
    );

    (
        StatusCode::CREATED,
        Json(json!({
            "client_id": client_id,
            "redirect_uris": redirect_uris,
            "token_endpoint_auth_method": "none",
        })),
    )
}

/// Query params for GET /authorize
#[derive(serde::Deserialize)]
struct AuthorizeParams {
    response_type: Option<String>,
    client_id: Option<String>,
    redirect_uri: Option<String>,
    state: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
}

/// GET /authorize — Show the authorization page
async fn handle_oauth_authorize(
    Query(params): Query<AuthorizeParams>,
) -> impl IntoResponse {
    let redirect_uri = params.redirect_uri.unwrap_or_default();
    let client_state = params.state.unwrap_or_default();
    let code_challenge = params.code_challenge.unwrap_or_default();
    let code_challenge_method = params.code_challenge_method.unwrap_or_default();

    eprintln!("[oauth/authorize] redirect_uri={} state={} code_challenge={} code_challenge_method={}",
        redirect_uri, client_state,
        if code_challenge.is_empty() { "(none)" } else { &code_challenge },
        if code_challenge_method.is_empty() { "(none)" } else { &code_challenge_method });

    Html(format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>Treeline — Authorize</title>
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <style>
        body {{ font-family: -apple-system, system-ui, sans-serif; max-width: 400px; margin: 80px auto; padding: 0 20px; color: #1a1a1a; }}
        h1 {{ font-size: 1.4em; margin-bottom: 0.5em; }}
        p {{ color: #666; line-height: 1.5; }}
        input[type=text] {{ width: 100%; padding: 10px; border: 1px solid #ccc; border-radius: 6px; font-size: 16px; box-sizing: border-box; font-family: monospace; }}
        button {{ width: 100%; padding: 12px; background: #1a1a1a; color: white; border: none; border-radius: 6px; font-size: 16px; cursor: pointer; margin-top: 12px; }}
        button:hover {{ background: #333; }}
        .error {{ color: #dc2626; display: none; margin-top: 8px; }}
    </style>
</head>
<body>
    <h1>Connect to Treeline</h1>
    <p>An application wants to access your Treeline hub. Enter your hub token to authorize.</p>
    <form method="POST" action="/authorize">
        <input type="hidden" name="redirect_uri" value="{redirect_uri}">
        <input type="hidden" name="state" value="{client_state}">
        <input type="hidden" name="code_challenge" value="{code_challenge}">
        <input type="hidden" name="code_challenge_method" value="{code_challenge_method}">
        <input type="text" name="hub_token" placeholder="Paste your hub token" autofocus required>
        <button type="submit">Authorize</button>
    </form>
</body>
</html>"#
    ))
}

/// Form data from the authorize page
#[derive(serde::Deserialize)]
struct AuthorizeForm {
    hub_token: String,
    redirect_uri: String,
    state: String,
    code_challenge: Option<String>,
    #[allow(dead_code)]
    code_challenge_method: Option<String>,
}

/// POST /authorize — Validate token and redirect with auth code
async fn handle_oauth_authorize_submit(
    State(state): State<Arc<AppState>>,
    axum::Form(form): axum::Form<AuthorizeForm>,
) -> impl IntoResponse {
    eprintln!("[oauth/authorize POST] redirect_uri={} state={} code_challenge={:?}",
        form.redirect_uri, form.state, form.code_challenge);

    // Validate the hub token
    let valid = HubService::validate_token(&state.treeline_dir, &form.hub_token)
        .unwrap_or(false);

    if !valid {
        return Html(
            r#"<!DOCTYPE html>
<html>
<head>
    <title>Treeline — Authorization Failed</title>
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <style>
        body { font-family: -apple-system, system-ui, sans-serif; max-width: 400px; margin: 80px auto; padding: 0 20px; }
        h1 { font-size: 1.4em; color: #dc2626; }
        a { color: #1a1a1a; }
    </style>
</head>
<body>
    <h1>Invalid token</h1>
    <p>The hub token you entered is incorrect. Check the output of <code>tl serve</code> for the correct token.</p>
    <p><a href="javascript:history.back()">Try again</a></p>
</body>
</html>"#
                .to_string(),
        )
            .into_response();
    }

    // Generate a single-use auth code
    let code = generate_random_id();

    let mut codes = state.auth_codes.lock().await;
    codes.insert(
        code.clone(),
        AuthCodeEntry {
            redirect_uri: form.redirect_uri.clone(),
            code_challenge: form.code_challenge,
            _created_at: std::time::Instant::now(),
        },
    );

    // Redirect back to the client with the auth code
    let separator = if form.redirect_uri.contains('?') {
        "&"
    } else {
        "?"
    };
    let redirect_url = format!(
        "{}{}code={}&state={}",
        form.redirect_uri, separator, code, form.state
    );

    // Use 302 Found (not 307) per OAuth spec
    (StatusCode::FOUND, [(axum::http::header::LOCATION, redirect_url)]).into_response()
}

/// POST /token — Exchange auth code for access token
#[derive(serde::Deserialize)]
struct TokenRequest {
    grant_type: String,
    code: Option<String>,
    code_verifier: Option<String>,
    #[allow(dead_code)]
    redirect_uri: Option<String>,
    #[allow(dead_code)]
    client_id: Option<String>,
}

async fn handle_oauth_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    eprintln!("[oauth/token] Content-Type: {:?}", headers.get("content-type"));
    eprintln!("[oauth/token] Body: {}", String::from_utf8_lossy(&body));

    // Parse as form-urlencoded (OAuth 2.1 spec) or JSON (some clients)
    let form: TokenRequest = if headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("application/json"))
        .unwrap_or(false)
    {
        match serde_json::from_slice(&body) {
            Ok(f) => f,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "invalid_request", "error_description": format!("Invalid JSON: {}", e)})),
                ).into_response();
            }
        }
    } else {
        match serde_urlencoded::from_bytes(&body) {
            Ok(f) => f,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "invalid_request", "error_description": format!("Invalid form data: {}", e)})),
                ).into_response();
            }
        }
    };
    if form.grant_type != "authorization_code" {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "unsupported_grant_type",
                "error_description": "Only authorization_code is supported",
            })),
        )
            .into_response();
    }

    let code = match &form.code {
        Some(c) => c.clone(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "invalid_request",
                    "error_description": "Missing code parameter",
                })),
            )
                .into_response()
        }
    };

    // Look up and consume the auth code (single-use)
    let entry = {
        let mut codes = state.auth_codes.lock().await;
        codes.remove(&code)
    };

    let entry = match entry {
        Some(e) => e,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "invalid_grant",
                    "error_description": "Invalid or expired authorization code",
                })),
            )
                .into_response()
        }
    };

    // Validate PKCE code_verifier if a code_challenge was provided
    if let Some(challenge) = &entry.code_challenge {
        let verifier = match &form.code_verifier {
            Some(v) => v,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "invalid_request",
                        "error_description": "Missing code_verifier",
                    })),
                )
                    .into_response()
            }
        };

        // S256: BASE64URL(SHA256(code_verifier)) == code_challenge
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let hash = hasher.finalize();
        let computed = base64_url_encode(&hash);

        if computed != *challenge {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "invalid_grant",
                    "error_description": "PKCE verification failed",
                })),
            )
                .into_response();
        }
    }

    // Issue the hub token as the access token
    let hub_token = HubService::load_or_create_token(&state.treeline_dir)
        .unwrap_or_default();

    (
        StatusCode::OK,
        Json(json!({
            "access_token": hub_token,
            "token_type": "Bearer",
        })),
    )
        .into_response()
}

// ============================================================================
// Auth check (Bearer token)
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

// ============================================================================
// Helpers
// ============================================================================

/// Derive the public base URL from request headers.
/// Respects X-Forwarded-Host/X-Forwarded-Proto (set by reverse proxies like ngrok/Caddy)
/// and falls back to the Host header.
fn base_url_from_headers(headers: &HeaderMap) -> String {
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");

    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get("host"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");

    format!("{}://{}", proto, host)
}

fn generate_random_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.gen::<u8>()).collect();
    hex::encode(bytes)
}

fn base64_url_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}
