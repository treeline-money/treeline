//! Serve command — expose Treeline as an HTTP hub.
//!
//! Two audiences:
//!   1. Owner path (you + your `tl hub` CLI) → `/api/push`, `/api/pull`, `/api/hash`,
//!      authenticated by the master hub token (`~/.treeline/hub-token`).
//!   2. Thin clients (Claude Desktop, claude.ai, etc.) → `/mcp`, authenticated
//!      by per-client OAuth access tokens minted from the flow in this file.
//!
//! OAuth 2.1 endpoints implement dynamic client registration, PKCE, and
//! refresh tokens. State is persisted via `treeline_core::services::oauth::OAuthStore`
//! so registered clients and issued tokens survive server restarts.
//!
//! Scopes: `read` (query/status/schema/skills_read/etc) and `write` (adds
//! query_write, sync, tag, demo, skills_write). The master token is only
//! accepted on `/api/*` — never on `/mcp`.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde_json::json;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use treeline_core::services::hub::HubService;
use treeline_core::services::oauth::{
    ExchangeError, OAuthStore, RefreshError, ValidateError, ValidatedToken,
};

use super::get_treeline_dir;
use super::mcp;

// ============================================================================
// Scopes
//
// Two scope families share the same OAuth machinery, distinguished only by
// audience and the routes they unlock:
//   - MCP scopes (`read`, `write`) — for thin clients on `/mcp` (Claude,
//     ChatGPT, etc.). Existing.
//   - Replication scopes (`pull`, `push`) — for Treeline desktops/CLIs on
//     `/api/{pull,push,hash}`. New. The CLI uses the device-code flow
//     because there's no browser to redirect back to.
//
// `/authorize` renders only the section relevant to whichever family the
// requesting client asked for, so an MCP client never sees pull/push and a
// device never sees read/write.
// ============================================================================

const SCOPE_READ: &str = "read";
const SCOPE_WRITE: &str = "write";
const SCOPE_PULL: &str = "pull";
const SCOPE_PUSH: &str = "push";

const ALL_VALID_SCOPES: &[&str] = &[SCOPE_READ, SCOPE_WRITE, SCOPE_PULL, SCOPE_PUSH];
const MCP_SCOPES: &[&str] = &[SCOPE_READ, SCOPE_WRITE];
const REPLICATE_SCOPES: &[&str] = &[SCOPE_PULL, SCOPE_PUSH];

/// MCP tools that mutate state. Calling these requires `write` scope.
const WRITE_TOOLS: &[&str] = &["query_write", "sync", "tag", "demo", "skills_write"];

/// MCP tools that read or write the DuckDB database. Calling these on a hub
/// with no `treeline.duckdb` yet returns a JSON-RPC isError result rather than
/// silently creating an empty DB. Tools not in this list (`version`,
/// `encryption_status`, `skills_*`, `demo`) work without a database.
const TOOLS_REQUIRING_DB: &[&str] =
    &["status", "query", "query_write", "sync", "tag", "doctor", "schema"];

fn has_scope(scopes: &[String], needed: &str) -> bool {
    scopes.iter().any(|s| s == needed)
}

/// Family the requesting client falls into. Drives which section of the
/// authorize page renders and which routes the issued token can hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScopeFamily {
    Mcp,
    Replicate,
    /// No recognizable scopes — happens when a client requested only
    /// unknown scope strings. We default to MCP read for backward compat
    /// with the existing flow.
    Unknown,
}

fn scope_family(scopes: &[String]) -> ScopeFamily {
    let has_mcp = scopes.iter().any(|s| MCP_SCOPES.contains(&s.as_str()));
    let has_replicate = scopes.iter().any(|s| REPLICATE_SCOPES.contains(&s.as_str()));
    match (has_mcp, has_replicate) {
        (true, false) => ScopeFamily::Mcp,
        (false, true) => ScopeFamily::Replicate,
        _ => ScopeFamily::Unknown,
    }
}

/// Parse the space-delimited `scope` param from the authorize form. Unknown
/// scopes are dropped. Empty → defaults to `read` (preserves existing MCP
/// fallback behavior).
fn parse_requested_scopes(raw: &str) -> Vec<String> {
    let valid: Vec<String> = raw
        .split_whitespace()
        .filter(|s| ALL_VALID_SCOPES.contains(s))
        .map(|s| s.to_string())
        .collect();
    if valid.is_empty() {
        vec![SCOPE_READ.to_string()]
    } else {
        valid
    }
}

// ============================================================================
// App state
// ============================================================================

/// Shared state handed to every request handler. Constructed once at startup
/// (or per test) and cloned via `Arc`.
pub struct AppState {
    pub hub_service: HubService,
    pub treeline_dir: PathBuf,
    /// RwLock allowing concurrent reads and exclusive writes against the DB.
    pub db_lock: RwLock<()>,
    pub oauth_store: Arc<OAuthStore>,
}

impl AppState {
    pub fn new(
        treeline_dir: PathBuf,
        hub_service: HubService,
        oauth_store: Arc<OAuthStore>,
    ) -> Self {
        Self {
            hub_service,
            treeline_dir,
            db_lock: RwLock::new(()),
            oauth_store,
        }
    }
}

// ============================================================================
// Router construction — shared by `run()` and integration tests
// ============================================================================

pub fn build_app(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
        .allow_headers(Any)
        .expose_headers([axum::http::header::HeaderName::from_static(
            "mcp-session-id",
        )]);

    Router::new()
        // Core sync endpoints (owner path).
        .route("/health", get(health))
        .route(
            "/api/push",
            post(handle_push).layer(DefaultBodyLimit::max(500 * 1024 * 1024)),
        )
        .route("/api/pull", get(handle_pull))
        .route("/api/hash", get(handle_hash))
        // MCP over HTTP (thin-client path).
        .route(
            "/mcp",
            post(handle_mcp)
                .get(handle_mcp_get)
                .delete(handle_mcp_delete),
        )
        // OAuth 2.1 endpoints.
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
        .route("/revoke", post(handle_oauth_revoke))
        .route("/device/code", post(handle_device_code))
        // Admin: list connected devices and apps. Master-token gated; Pro
        // calls this with master from vault. Self-hosters can curl it.
        .route("/api/clients", get(handle_list_clients))
        // Admin: revoke a client by its client_id. Wipes all access +
        // refresh tokens issued to that client. Same master-gating.
        .route("/api/clients/{client_id}", delete(handle_revoke_client))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub fn run(host: &str, port: u16) -> Result<()> {
    init_tracing();

    let treeline_dir = get_treeline_dir();
    std::fs::create_dir_all(&treeline_dir)?;

    let master_token = HubService::load_or_create_token(&treeline_dir)?;
    let hub_service = HubService::new(treeline_dir.clone(), "treeline.duckdb".to_string());
    let has_db = hub_service.has_database();
    let oauth_store = Arc::new(OAuthStore::new(treeline_dir.clone()));

    let addr: SocketAddr = format!("{}:{}", host, port).parse()?;

    let state = Arc::new(AppState::new(treeline_dir, hub_service, oauth_store));

    eprintln!("Treeline hub starting on http://{}", addr);
    eprintln!("Master hub token: {}", master_token);
    eprintln!("(only used at the /authorize page when linking a new device or app)");
    if !has_db {
        eprintln!();
        eprintln!("No database yet. Waiting for first push.");
    }
    eprintln!();
    eprintln!("Link a device with:");
    eprintln!("  tl hub link --url http://{}", addr);

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let app = build_app(state);
        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;
        Ok(())
    })
}

// ============================================================================
// Core handlers
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
) -> axum::response::Response {
    if let Err(e) = require_replicate_scope(&state.oauth_store, &headers, SCOPE_PUSH) {
        return e;
    }

    let base_hash = params.base_hash.clone();
    let _lock = state.db_lock.write().await;

    match state.hub_service.accept_push(&body, base_hash.as_deref()) {
        Ok(treeline_core::services::PushOutcome::Accepted {
            backup_name,
            bytes_received,
            new_hash,
        }) => (
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
) -> axum::response::Response {
    if let Err(e) = require_replicate_scope(&state.oauth_store, &headers, SCOPE_PULL) {
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
) -> axum::response::Response {
    // `pull` is enough to read the hash — it's a precondition check for a
    // pull operation, not a write.
    if let Err(e) = require_replicate_scope(&state.oauth_store, &headers, SCOPE_PULL) {
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
) -> axum::response::Response {
    let validated = match require_oauth_token(&state.oauth_store, &headers) {
        Ok(v) => v,
        Err(e) => return e,
    };

    // JSON-RPC errors travel inside HTTP 200 — clients route by error code,
    // not status code. The MCP spec is explicit on this; Claude treats non-200
    // here as a transport failure rather than reading the JSON-RPC error body.
    let req: mcp::JsonRpcRequest = match serde_json::from_slice(&body) {
        Ok(req) => req,
        Err(e) => {
            let resp = mcp::JsonRpcResponse::error(
                serde_json::Value::Null,
                -32700,
                format!("Parse error: {}", e),
            );
            return (StatusCode::OK, Json(resp)).into_response();
        }
    };

    let is_write = is_write_request(&req);

    if is_write && !has_scope(&validated.scopes, SCOPE_WRITE) {
        return insufficient_scope_response(SCOPE_WRITE);
    }
    if !has_scope(&validated.scopes, SCOPE_READ)
        && !has_scope(&validated.scopes, SCOPE_WRITE)
    {
        return insufficient_scope_response(SCOPE_READ);
    }

    // Reject *only* data-requiring tool calls when no DB exists. `initialize`,
    // `tools/list`, and tools that don't need DuckDB (`version`,
    // `encryption_status`, `skills_*`, `demo`) still work, so thin clients can
    // connect and discover capabilities before the user has pushed a database.
    if requires_database(&req) && !state.hub_service.has_database() {
        return no_database_response(&req);
    }

    let response = if is_write {
        let _lock = state.db_lock.write().await;
        let resp = mcp::handle_request(&req);
        // Recompute hash after writes so conflict detection stays current.
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

fn requires_database(req: &mcp::JsonRpcRequest) -> bool {
    if req.method != "tools/call" {
        return false;
    }
    req.params
        .as_ref()
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .map(|name| TOOLS_REQUIRING_DB.contains(&name))
        .unwrap_or(false)
}

fn no_database_response(req: &mcp::JsonRpcRequest) -> axum::response::Response {
    let id = req.id.clone().unwrap_or(serde_json::Value::Null);
    let resp = mcp::JsonRpcResponse::success(
        id,
        json!({
            "content": [{
                "type": "text",
                "text": "No database on hub yet. Push a database first with `tl hub push`.",
            }],
            "isError": true,
        }),
    );
    (StatusCode::OK, Json(resp)).into_response()
}

fn is_write_request(req: &mcp::JsonRpcRequest) -> bool {
    if req.method != "tools/call" {
        return false;
    }
    req.params
        .as_ref()
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .map(|name| WRITE_TOOLS.contains(&name))
        .unwrap_or(false)
}

async fn handle_mcp_get(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> axum::response::Response {
    if let Err(e) = require_oauth_token(&state.oauth_store, &headers) {
        return e;
    }
    // TODO(hub-sse): Full Streamable-HTTP SSE streaming for server->client events.
    // For now, acknowledge the session so Claude Desktop's GET handshake doesn't error.
    StatusCode::OK.into_response()
}

async fn handle_mcp_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> axum::response::Response {
    if let Err(e) = require_oauth_token(&state.oauth_store, &headers) {
        return e;
    }
    StatusCode::OK.into_response()
}

// ============================================================================
// OAuth 2.1
// ============================================================================

async fn handle_oauth_protected_resource(headers: HeaderMap) -> axum::response::Response {
    let base = base_url_from_headers(&headers);
    Json(json!({
        "resource": format!("{}/mcp", base),
        "authorization_servers": [base],
        "scopes_supported": [SCOPE_READ, SCOPE_WRITE],
    }))
    .into_response()
}

async fn handle_oauth_metadata(headers: HeaderMap) -> axum::response::Response {
    let base = base_url_from_headers(&headers);
    Json(json!({
        "issuer": base,
        "authorization_endpoint": format!("{}/authorize", base),
        "token_endpoint": format!("{}/token", base),
        "registration_endpoint": format!("{}/register", base),
        "revocation_endpoint": format!("{}/revoke", base),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "token_endpoint_auth_methods_supported": ["none"],
        "code_challenge_methods_supported": ["S256"],
        "scopes_supported": [SCOPE_READ, SCOPE_WRITE],
    }))
    .into_response()
}

#[derive(serde::Deserialize)]
struct RegisterRequest {
    #[serde(default)]
    redirect_uris: Vec<String>,
    #[serde(default)]
    client_name: Option<String>,
}

async fn handle_oauth_register(
    State(state): State<Arc<AppState>>,
    body: Bytes,
) -> axum::response::Response {
    let req: RegisterRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "invalid_request",
                    "error_description": format!("Invalid JSON: {}", e),
                })),
            )
                .into_response();
        }
    };

    let client = match state
        .oauth_store
        .register_client(req.redirect_uris.clone(), req.client_name.clone())
    {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "server_error",
                    "error_description": e.to_string(),
                })),
            )
                .into_response();
        }
    };

    (
        StatusCode::CREATED,
        Json(json!({
            "client_id": client.client_id,
            "client_name": client.client_name,
            "redirect_uris": client.redirect_uris,
            "token_endpoint_auth_method": "none",
        })),
    )
        .into_response()
}

#[derive(serde::Deserialize)]
struct AuthorizeParams {
    #[allow(dead_code)]
    response_type: Option<String>,
    client_id: Option<String>,
    redirect_uri: Option<String>,
    state: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
    scope: Option<String>,
    /// Device-code flow: when present, the page is the completion screen
    /// for a CLI's pending session, not a redirect-flow start. The session
    /// already carries client_id and scopes; we look them up server-side
    /// rather than trusting query params.
    user_code: Option<String>,
}

async fn handle_oauth_authorize(
    State(state): State<Arc<AppState>>,
    Query(params): Query<AuthorizeParams>,
) -> axum::response::Response {
    // Branch 1: device-code completion. The CLI has a pending session
    // tied to user_code; we look up its client+scopes from the store, not
    // from the query string.
    if let Some(user_code) = params.user_code.as_deref().filter(|s| !s.is_empty()) {
        return render_device_authorize_page(&state, user_code).await;
    }

    // Branch 2: classic authorization-code redirect flow (unchanged behavior).
    let redirect_uri = params.redirect_uri.unwrap_or_default();
    let client_state = params.state.unwrap_or_default();
    let code_challenge = params.code_challenge.unwrap_or_default();
    let code_challenge_method = params.code_challenge_method.unwrap_or_default();
    let client_id = params.client_id.unwrap_or_default();
    let scope = params.scope.unwrap_or_else(|| SCOPE_READ.to_string());

    let requested = parse_requested_scopes(&scope);
    let scope_items = render_scope_items(&requested);
    let (heading, lead) = authorize_copy_for_family(scope_family(&requested));

    let body = format!(
        r#"<h1>{heading}</h1>
<p class="lead">{lead}</p>
<div class="scopes">
  <div class="scopes-label">Requested permissions</div>
  <div class="scope-list">{scope_items}</div>
</div>
<form method="POST" action="/authorize" autocomplete="off">
  <input type="hidden" name="redirect_uri" value="{redirect_uri}">
  <input type="hidden" name="state" value="{client_state}">
  <input type="hidden" name="code_challenge" value="{code_challenge}">
  <input type="hidden" name="code_challenge_method" value="{code_challenge_method}">
  <input type="hidden" name="client_id" value="{client_id}">
  <input type="hidden" name="scope" value="{scope}">
  <label for="client_name">Name this client <span class="muted">(optional)</span></label>
  <input id="client_name" type="text" name="client_name" placeholder="e.g. Claude Desktop on laptop">
  <label for="hub_token">Hub token</label>
  <input id="hub_token" class="mono" type="text" name="hub_token" placeholder="Paste your hub token" autocomplete="off" autofocus required>
  <button type="submit">Authorize</button>
</form>
<p class="hint">Only authorize devices and apps you trust. You can revoke access any time with <code>tl hub tokens revoke</code>.</p>"#
    );

    Html(render_page("Authorize", &body)).into_response()
}

/// `(heading, lead)` for the authorize page based on what kind of client
/// is asking. Self-hosters see "Connect a new app" for an MCP client and
/// "Link a device" for a Treeline desktop/CLI.
fn authorize_copy_for_family(family: ScopeFamily) -> (&'static str, &'static str) {
    match family {
        ScopeFamily::Mcp => (
            "Connect a new app",
            "An app wants to access your Treeline hub. Authorize it by pasting your hub token below.",
        ),
        ScopeFamily::Replicate => (
            "Link a device",
            "A Treeline device wants to sync with this hub. Authorize it by pasting your hub token below.",
        ),
        ScopeFamily::Unknown => (
            "Authorize",
            "A client wants to access your Treeline hub. Authorize it by pasting your hub token below.",
        ),
    }
}

/// Render the scope checklist for the authorize page. Each scope appears
/// once with a friendly label/description; unknown scope strings are
/// silently dropped. By design only one family is shown — if a client asks
/// for both, we show whichever family is actually requested first.
fn render_scope_items(scopes: &[String]) -> String {
    scopes
        .iter()
        .filter_map(|s| {
            let (name, desc) = match s.as_str() {
                SCOPE_READ => (
                    "Read",
                    "View your accounts, transactions, balances, and skills.",
                ),
                SCOPE_WRITE => (
                    "Write",
                    "Modify data, run bank syncs, tag transactions, and edit skills.",
                ),
                SCOPE_PULL => (
                    "Pull",
                    "Pull database bundles from this hub.",
                ),
                SCOPE_PUSH => (
                    "Push",
                    "Push database bundles to this hub.",
                ),
                _ => return None,
            };
            Some(format!(
                r#"<div class="scope-item"><span class="scope-check" aria-hidden="true">✓</span><div class="scope-body"><div class="scope-name">{}</div><div class="scope-desc">{}</div></div></div>"#,
                name, desc
            ))
        })
        .collect()
}

/// Page rendered when the user opened a `verification_uri_complete` URL
/// from a CLI device-code flow. The session knows the client and scopes;
/// the user just confirms by master-pasting.
async fn render_device_authorize_page(
    state: &Arc<AppState>,
    user_code: &str,
) -> axum::response::Response {
    let info = match state.oauth_store.find_pending_device_session(user_code) {
        Ok(Some(info)) => info,
        Ok(None) => {
            let body = r#"<h1 class="error-heading">Code not found</h1>
<p class="lead">That sign-in code is unknown or expired. Run <code>tl hub link</code> again to get a fresh one.</p>"#;
            return Html(render_page("Sign in", body)).into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };

    let scope_items = render_scope_items(&info.scopes);
    let (heading, lead) = authorize_copy_for_family(scope_family(&info.scopes));
    let suggested_name = info
        .client_name
        .as_deref()
        .unwrap_or("Treeline device")
        .to_string();

    let body = format!(
        r#"<h1>{heading}</h1>
<p class="lead">{lead}</p>
<p class="muted">Sign-in code: <code>{user_code}</code></p>
<div class="scopes">
  <div class="scopes-label">Requested permissions</div>
  <div class="scope-list">{scope_items}</div>
</div>
<form method="POST" action="/authorize" autocomplete="off">
  <input type="hidden" name="user_code" value="{user_code}">
  <label for="client_name">Name this device <span class="muted">(optional)</span></label>
  <input id="client_name" type="text" name="client_name" value="{suggested_name}">
  <label for="hub_token">Hub master token</label>
  <input id="hub_token" class="mono" type="text" name="hub_token" placeholder="Paste your master token" autocomplete="off" autofocus required>
  <button type="submit">Authorize</button>
</form>
<p class="hint">After authorizing, return to your terminal — <code>tl hub link</code> will finish automatically.</p>"#
    );

    Html(render_page("Sign in", &body)).into_response()
}

#[derive(serde::Deserialize)]
struct AuthorizeForm {
    hub_token: String,
    /// Set on the redirect (authorization-code) flow path. Empty/missing
    /// when the form came from the device-code completion page.
    #[serde(default)]
    redirect_uri: String,
    #[serde(default)]
    state: String,
    code_challenge: Option<String>,
    #[allow(dead_code)]
    code_challenge_method: Option<String>,
    client_id: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    client_name: Option<String>,
    /// Set on the device-code completion path. When present, this submit
    /// completes a CLI's pending session instead of issuing an
    /// authorization code for redirect.
    #[serde(default)]
    user_code: Option<String>,
}

async fn handle_oauth_authorize_submit(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::Form(form): axum::Form<AuthorizeForm>,
) -> axum::response::Response {
    let wants_json = wants_json_response(&headers);

    // Validate the master hub token. This is what proves the user owns the hub.
    let valid =
        HubService::validate_token(&state.treeline_dir, &form.hub_token).unwrap_or(false);

    if !valid {
        if wants_json {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "invalid_token",
                    "error_description": "The hub token is incorrect.",
                })),
            )
                .into_response();
        }
        let body = r#"<h1 class="error-heading">Invalid token</h1>
<p class="lead">The hub token you entered is incorrect. Double-check it and try again.</p>
<p class="hint">Retrieve your token with <code>fly ssh console -a treeline-hub -C "cat /data/hub-token"</code> or from your local <code>~/.treeline/hub-token</code> if you're self-hosting.</p>
<p><a class="backlink" href="javascript:history.back()">← Go back</a></p>"#;
        return Html(render_page("Authorization failed", body)).into_response();
    }

    // Branch: device-code completion. Renames the pending session's client
    // (so the device shows up under a friendly name in `/api/clients`),
    // mints tokens, and tells the user to switch back to their terminal.
    if let Some(user_code) = form.user_code.as_deref().filter(|s| !s.is_empty()) {
        return complete_device_authorization(state, user_code, form.client_name.clone(), wants_json).await;
    }

    // Resolve the client_id. If the thin client didn't register (some OAuth
    // clients skip dynamic registration), synthesize a client record on the fly.
    let client_id = match form.client_id.as_deref() {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => {
            match state.oauth_store.register_client(
                vec![form.redirect_uri.clone()],
                form.client_name.clone(),
            ) {
                Ok(c) => c.client_id,
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": "server_error", "error_description": e.to_string()})),
                    )
                        .into_response();
                }
            }
        }
    };

    let scopes = parse_requested_scopes(form.scope.as_deref().unwrap_or(""));

    let code = match state.oauth_store.issue_authorization_code(
        &client_id,
        &form.redirect_uri,
        form.code_challenge.clone(),
        scopes,
    ) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "server_error", "error_description": e.to_string()})),
            )
                .into_response();
        }
    };

    let separator = if form.redirect_uri.contains('?') { "&" } else { "?" };
    let redirect_url = format!(
        "{}{}code={}&state={}",
        form.redirect_uri, separator, code, form.state
    );

    (StatusCode::FOUND, [(axum::http::header::LOCATION, redirect_url)]).into_response()
}

/// Authorize a CLI's pending device-code session. Looks up the session,
/// renames its client to the user-supplied name, mints tokens, and shows a
/// "you can close this tab" page.
async fn complete_device_authorization(
    state: Arc<AppState>,
    user_code: &str,
    client_name: Option<String>,
    wants_json: bool,
) -> axum::response::Response {
    // Look up the session to learn (a) its client_id (so we can rename it
    // for the dashboard) and (b) the scopes the CLI requested (so the
    // minted tokens grant exactly those, no more).
    let info = match state.oauth_store.find_pending_device_session(user_code) {
        Ok(Some(i)) => i,
        Ok(None) => {
            if wants_json {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({
                        "error": "code_not_found",
                        "error_description": "That sign-in code is unknown or expired.",
                    })),
                )
                    .into_response();
            }
            let body = r#"<h1 class="error-heading">Code not found</h1>
<p class="lead">That sign-in code is unknown or expired. Run <code>tl hub link</code> again to get a fresh one.</p>"#;
            return Html(render_page("Sign in", body)).into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };

    // Rename the client if the user provided a fresh name. Skip if it's
    // the same we suggested (just trim/empty checks).
    if let Some(name) = client_name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        if Some(name) != info.client_name.as_deref() {
            if let Err(e) = state.oauth_store.set_client_name(&info.client_id, name) {
                tracing::warn!("failed to rename client {}: {}", info.client_id, e);
            }
        }
    }

    if let Err(e) = state
        .oauth_store
        .authorize_device_session(user_code, info.scopes)
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response();
    }

    if wants_json {
        return (
            StatusCode::OK,
            Json(json!({"status": "linked"})),
        )
            .into_response();
    }
    let body = r#"<h1>Device linked</h1>
<p class="lead">You can close this tab — the terminal will finish automatically.</p>"#;
    Html(render_page("Linked", body)).into_response()
}

/// Inspect the request's `Accept` header for a JSON content negotiation.
/// Browsers send `text/html, ...` and get the rendered page; programmatic
/// callers (Pro's link orchestrator) send `application/json` and get a
/// machine-readable response with proper status codes.
fn wants_json_response(headers: &HeaderMap) -> bool {
    headers
        .get(axum::http::header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            // Fast path: explicit "application/json" anywhere in the list,
            // and not preferring text/html over it. Conservative — when in
            // doubt, return HTML to keep browsers happy.
            v.contains("application/json") && !v.starts_with("text/html")
        })
        .unwrap_or(false)
}

#[derive(serde::Deserialize)]
struct TokenRequest {
    grant_type: String,
    // authorization_code grant
    code: Option<String>,
    code_verifier: Option<String>,
    #[allow(dead_code)]
    redirect_uri: Option<String>,
    #[allow(dead_code)]
    client_id: Option<String>,
    // refresh_token grant
    refresh_token: Option<String>,
    // device_code grant (RFC 8628). Both forms of the device_code grant
    // string land here so the CLI can use whichever it prefers.
    device_code: Option<String>,
}

async fn handle_oauth_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    let form: TokenRequest = match parse_form_or_json::<TokenRequest>(&headers, &body) {
        Ok(f) => f,
        Err(resp) => return resp,
    };

    match form.grant_type.as_str() {
        "authorization_code" => handle_grant_authorization_code(state, form).await,
        "refresh_token" => handle_grant_refresh_token(state, form).await,
        // RFC 8628 specifies the verbose URN form. Accept the short form
        // too — it's what most CLIs send and what's least error-prone for
        // anyone reading config / debugging.
        "urn:ietf:params:oauth:grant-type:device_code" | "device_code" => {
            handle_grant_device_code(state, form).await
        }
        _ => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "unsupported_grant_type",
                "error_description": "Only authorization_code, refresh_token, and device_code are supported",
            })),
        )
            .into_response(),
    }
}

async fn handle_grant_authorization_code(
    state: Arc<AppState>,
    form: TokenRequest,
) -> axum::response::Response {
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
                .into_response();
        }
    };

    match state
        .oauth_store
        .exchange_authorization_code(&code, form.code_verifier.as_deref())
    {
        Ok(pair) => (
            StatusCode::OK,
            Json(json!({
                "access_token": pair.access_token,
                "refresh_token": pair.refresh_token,
                "token_type": "Bearer",
                "expires_in": pair.expires_in,
                "scope": pair.scopes.join(" "),
            })),
        )
            .into_response(),
        Err(ExchangeError::UnknownCode) | Err(ExchangeError::Expired) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "invalid_grant",
                "error_description": "Invalid or expired authorization code",
            })),
        )
            .into_response(),
        Err(ExchangeError::MissingVerifier) | Err(ExchangeError::PkceFailed) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "invalid_grant",
                "error_description": "PKCE verification failed",
            })),
        )
            .into_response(),
        Err(ExchangeError::Io(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "server_error", "error_description": e})),
        )
            .into_response(),
    }
}

async fn handle_grant_refresh_token(
    state: Arc<AppState>,
    form: TokenRequest,
) -> axum::response::Response {
    let refresh = match &form.refresh_token {
        Some(r) => r.clone(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "invalid_request",
                    "error_description": "Missing refresh_token parameter",
                })),
            )
                .into_response();
        }
    };

    match state.oauth_store.refresh_access_token(&refresh) {
        Ok(pair) => (
            StatusCode::OK,
            Json(json!({
                "access_token": pair.access_token,
                "refresh_token": pair.refresh_token,
                "token_type": "Bearer",
                "expires_in": pair.expires_in,
                "scope": pair.scopes.join(" "),
            })),
        )
            .into_response(),
        Err(RefreshError::Unknown) | Err(RefreshError::Expired) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "invalid_grant",
                "error_description": "Invalid or expired refresh token",
            })),
        )
            .into_response(),
        Err(RefreshError::Io(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "server_error", "error_description": e})),
        )
            .into_response(),
    }
}

async fn handle_grant_device_code(
    state: Arc<AppState>,
    form: TokenRequest,
) -> axum::response::Response {
    let device_code = match &form.device_code {
        Some(c) => c.clone(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "invalid_request",
                    "error_description": "Missing device_code parameter",
                })),
            )
                .into_response();
        }
    };

    use treeline_core::services::oauth::DeviceCodeError;
    match state.oauth_store.poll_device_token(&device_code) {
        Ok(pair) => (
            StatusCode::OK,
            Json(json!({
                "access_token": pair.access_token,
                "refresh_token": pair.refresh_token,
                "token_type": "Bearer",
                "expires_in": pair.expires_in,
                "scope": pair.scopes.join(" "),
            })),
        )
            .into_response(),
        // RFC 8628: each is a 400 with a specific error string. CLIs key
        // off `error` (not status), so the strings are load-bearing.
        Err(DeviceCodeError::AuthorizationPending) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "authorization_pending"})),
        )
            .into_response(),
        Err(DeviceCodeError::Expired) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "expired_token"})),
        )
            .into_response(),
        Err(DeviceCodeError::AccessDenied) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "access_denied"})),
        )
            .into_response(),
        Err(DeviceCodeError::Unknown) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid_grant"})),
        )
            .into_response(),
        Err(DeviceCodeError::Io(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "server_error", "error_description": e})),
        )
            .into_response(),
    }
}

// ============================================================================
// Device-code initiation (`POST /device/code`)
//
// First step of RFC 8628. CLI POSTs `client_id` + `scope`; server returns
// the codes the CLI prints and starts polling against. Public endpoint —
// no auth, just a way for a CLI to register intent. The actual proof of
// ownership happens at the browser-side `/authorize` step.
// ============================================================================

#[derive(serde::Deserialize)]
struct DeviceCodeRequest {
    client_id: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    /// Optional name from the CLI (e.g. hostname) so the authorize page
    /// can pre-fill it when registering the client. If omitted, falls back
    /// to "Treeline device".
    #[serde(default)]
    client_name: Option<String>,
}

async fn handle_device_code(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    let form: DeviceCodeRequest = match parse_form_or_json::<DeviceCodeRequest>(&headers, &body) {
        Ok(f) => f,
        Err(resp) => return resp,
    };

    let scopes = parse_requested_scopes(form.scope.as_deref().unwrap_or(""));

    // Either reuse the supplied client_id (if the CLI did dynamic
    // registration first) or synthesize one — most CLIs won't bother with
    // a separate /register call. Either way the issued tokens carry a
    // stable client_id so revocation works.
    let client_id = match form.client_id.as_deref() {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => {
            let name = form
                .client_name
                .clone()
                .unwrap_or_else(|| "Treeline device".to_string());
            match state.oauth_store.register_client(vec![], Some(name)) {
                Ok(c) => c.client_id,
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": "server_error", "error_description": e.to_string()})),
                    )
                        .into_response();
                }
            }
        }
    };

    let auth = match state
        .oauth_store
        .start_device_authorization(&client_id, scopes)
    {
        Ok(a) => a,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "server_error", "error_description": e.to_string()})),
            )
                .into_response();
        }
    };

    let base = base_url_from_headers(&headers);
    let verification_uri = format!("{}/authorize", base);
    let verification_uri_complete = format!(
        "{}/authorize?user_code={}",
        base,
        urlencoding_encode(&auth.user_code)
    );

    (
        StatusCode::OK,
        Json(json!({
            "device_code": auth.device_code,
            "user_code": auth.user_code,
            "verification_uri": verification_uri,
            "verification_uri_complete": verification_uri_complete,
            "expires_in": auth.expires_in,
            "interval": auth.interval,
        })),
    )
        .into_response()
}

// Minimal URL-encoder for the user_code (alphanumerics + dash). Avoids
// pulling a fresh crate just for this — the alphabet is constrained.
fn urlencoding_encode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' => c.to_string(),
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}

// ============================================================================
// `/api/clients` — admin listing
//
// Returns one row per registered OAuth client that currently has a valid
// access token. Master-token gated; intended for Pro to render its
// dashboard, but a self-hoster can curl it too.
// ============================================================================

async fn handle_list_clients(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> axum::response::Response {
    if let Err(e) = require_master_token(&state.treeline_dir, &headers) {
        return e;
    }

    match state.oauth_store.list_active_clients() {
        Ok(clients) => (StatusCode::OK, Json(json!({ "clients": clients }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// Revoke every access + refresh token issued to a single client. Idempotent:
/// the response is 204 even if the client_id has no live tokens (or never
/// existed). Master-token gated.
async fn handle_revoke_client(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(client_id): Path<String>,
) -> axum::response::Response {
    if let Err(e) = require_master_token(&state.treeline_dir, &headers) {
        return e;
    }

    match state.oauth_store.revoke_client(&client_id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
struct RevokeRequest {
    token: String,
    token_type_hint: Option<String>,
}

async fn handle_oauth_revoke(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    let form: RevokeRequest = match parse_form_or_json::<RevokeRequest>(&headers, &body) {
        Ok(f) => f,
        Err(resp) => return resp,
    };

    // RFC 7009: server MUST respond 200 even for unrecognized tokens.
    let hint = form.token_type_hint.as_deref();
    match hint {
        Some("refresh_token") => {
            let _ = state.oauth_store.revoke_refresh_token(&form.token);
        }
        Some("access_token") => {
            let _ = state.oauth_store.revoke_access_token(&form.token);
        }
        _ => {
            // Unknown hint — try both.
            let _ = state.oauth_store.revoke_access_token(&form.token);
            let _ = state.oauth_store.revoke_refresh_token(&form.token);
        }
    }

    StatusCode::OK.into_response()
}

// ============================================================================
// Branded HTML shell (shared by /authorize GET and its error page)
// ============================================================================

/// Treeline logo (mountain + sage trees), inlined so the page has no external
/// asset deps.
const BRAND_LOGO: &str = r##"<svg class="logo" viewBox="0 0 64 64" aria-hidden="true" xmlns="http://www.w3.org/2000/svg">
  <path d="M32 12 L20 35 L35 40 L44 35 Z" fill="#f5f5f4"/>
  <path d="M20 35 L35 40 L44 35 L54 52 L10 52 Z" fill="#4a8a63"/>
  <path d="M32 12 L54 52 L10 52 Z" stroke="#4a8a63" stroke-width="2.5" fill="none"/>
</svg>"##;

/// Palette + typography pulled from treeline.money's marketing site.
const BRAND_CSS: &str = r#":root {
  --bg: #f5f5f4;
  --bg-alt: #e7e7e6;
  --surface: #ffffff;
  --fg: #1c1c1c;
  --muted: #525252;
  --border: #c5c5c4;
  --accent: #4a8a63;
  --accent-hover: #3d7554;
  --accent-soft: rgba(74, 138, 99, 0.12);
  --danger: #b91c1c;
}
* { box-sizing: border-box; }
html, body { margin: 0; padding: 0; }
body {
  font-family: 'Outfit', ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  background: var(--bg);
  color: var(--fg);
  line-height: 1.55;
  min-height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
}
.card {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 14px;
  padding: 32px;
  max-width: 460px;
  width: 100%;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.04), 0 8px 24px rgba(0, 0, 0, 0.04);
}
.brand {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 24px;
  padding-bottom: 20px;
  border-bottom: 1px solid var(--border);
}
.logo { width: 32px; height: 32px; display: block; }
.wordmark {
  font-weight: 600;
  font-size: 1.05rem;
  letter-spacing: -0.01em;
}
h1 {
  font-size: 1.45rem;
  font-weight: 600;
  letter-spacing: -0.02em;
  line-height: 1.3;
  margin: 0 0 8px;
}
h1.error-heading { color: var(--danger); }
p { margin: 0 0 14px; }
.lead { color: var(--muted); margin: 0 0 22px; }
.hint {
  font-size: 0.87rem;
  color: var(--muted);
  margin-top: 22px;
  padding-top: 18px;
  border-top: 1px solid var(--border);
}
code {
  font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace;
  font-size: 0.88em;
  background: var(--bg-alt);
  padding: 1px 6px;
  border-radius: 4px;
}
.scopes {
  background: var(--accent-soft);
  border: 1px solid rgba(74, 138, 99, 0.25);
  border-radius: 10px;
  padding: 16px;
  margin: 0 0 24px;
}
.scopes-label {
  font-size: 0.72rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--accent-hover);
  margin: 0 0 12px;
}
.scope-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.scope-item {
  display: flex;
  gap: 12px;
  align-items: flex-start;
}
.scope-check {
  flex-shrink: 0;
  width: 20px;
  height: 20px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: var(--accent);
  color: #ffffff;
  border-radius: 50%;
  font-size: 12px;
  line-height: 1;
  font-weight: 700;
  margin-top: 1px;
}
.scope-body { flex: 1; min-width: 0; }
.scope-name {
  font-weight: 600;
  font-size: 0.95rem;
  color: var(--fg);
  letter-spacing: -0.005em;
}
.scope-desc {
  font-size: 0.87rem;
  color: var(--muted);
  margin-top: 2px;
  line-height: 1.45;
}
label {
  display: block;
  font-size: 0.88rem;
  font-weight: 500;
  color: var(--fg);
  margin: 16px 0 6px;
}
label .muted { color: var(--muted); font-weight: 400; }
input[type="text"] {
  width: 100%;
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  font-size: 15px;
  background: #ffffff;
  color: var(--fg);
  font-family: inherit;
  transition: border-color 0.15s ease, box-shadow 0.15s ease;
}
input[type="text"]::placeholder { color: #9a9a9a; }
input[type="text"]:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px rgba(74, 138, 99, 0.18);
}
input[type="text"].mono {
  font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
  font-size: 13px;
  letter-spacing: 0.02em;
}
button {
  width: 100%;
  padding: 12px 16px;
  background: var(--accent);
  color: #ffffff;
  border: none;
  border-radius: 8px;
  font-size: 15px;
  font-weight: 600;
  font-family: inherit;
  cursor: pointer;
  margin-top: 22px;
  transition: background 0.15s ease, transform 0.05s ease;
}
button:hover { background: var(--accent-hover); }
button:active { transform: translateY(1px); }
.backlink {
  display: inline-block;
  color: var(--accent);
  text-decoration: none;
  font-weight: 500;
  margin-top: 4px;
}
.backlink:hover { color: var(--accent-hover); text-decoration: underline; }
"#;

fn render_page(title: &str, body_html: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Treeline — {title}</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Outfit:wght@400;500;600&display=swap" rel="stylesheet">
<style>{css}</style>
</head>
<body>
<main class="card">
  <header class="brand">
    {logo}
    <span class="wordmark">Treeline</span>
  </header>
  {body}
</main>
</body>
</html>"#,
        title = title,
        css = BRAND_CSS,
        logo = BRAND_LOGO,
        body = body_html,
    )
}

// ============================================================================
// Auth middleware
// ============================================================================

/// Returns Ok(()) if the Bearer token matches the master hub token on disk.
fn require_master_token(
    treeline_dir: &std::path::Path,
    headers: &HeaderMap,
) -> std::result::Result<(), axum::response::Response> {
    let token = bearer_token(headers).ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Missing Authorization header"})),
        )
            .into_response()
    })?;

    match HubService::validate_token(treeline_dir, token) {
        Ok(true) => Ok(()),
        _ => Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Invalid master hub token"})),
        )
            .into_response()),
    }
}

/// Returns Ok(ValidatedToken) if the Bearer token is a valid per-client
/// OAuth access token. Master hub token is NOT accepted here.
fn require_oauth_token(
    store: &OAuthStore,
    headers: &HeaderMap,
) -> std::result::Result<ValidatedToken, axum::response::Response> {
    let token = bearer_token(headers).ok_or_else(|| unauthorized_response("Missing bearer token"))?;
    match store.validate_access_token(token) {
        Ok(v) => Ok(v),
        Err(ValidateError::Unknown) => Err(unauthorized_response("Unknown access token")),
        Err(ValidateError::Expired) => Err(unauthorized_response("Access token expired")),
    }
}

/// Gate `/api/{push,pull,hash}` on a pull-or-push-scoped device token.
/// Master tokens are NOT accepted — devices must be linked via the OAuth
/// device-code flow (`tl hub link`) to get a scoped token. Returns 401 if
/// the token is missing/invalid, 403 if it lacks the required scope.
fn require_replicate_scope(
    store: &OAuthStore,
    headers: &HeaderMap,
    needed_scope: &str,
) -> std::result::Result<(), axum::response::Response> {
    let validated = require_oauth_token(store, headers)?;
    if !has_scope(&validated.scopes, needed_scope) {
        return Err(insufficient_scope_response(needed_scope));
    }
    Ok(())
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
}

fn unauthorized_response(description: &str) -> axum::response::Response {
    (
        StatusCode::UNAUTHORIZED,
        [(
            axum::http::header::WWW_AUTHENTICATE,
            format!(r#"Bearer error="invalid_token", error_description="{}""#, description),
        )],
        Json(json!({"error": "invalid_token", "error_description": description})),
    )
        .into_response()
}

fn insufficient_scope_response(needed: &str) -> axum::response::Response {
    (
        StatusCode::FORBIDDEN,
        [(
            axum::http::header::WWW_AUTHENTICATE,
            format!(
                r#"Bearer error="insufficient_scope", scope="{}""#,
                needed
            ),
        )],
        Json(json!({
            "error": "insufficient_scope",
            "scope": needed,
        })),
    )
        .into_response()
}

// ============================================================================
// Helpers
// ============================================================================

fn parse_form_or_json<T: serde::de::DeserializeOwned>(
    headers: &HeaderMap,
    body: &Bytes,
) -> std::result::Result<T, axum::response::Response> {
    let is_json = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("application/json"))
        .unwrap_or(false);

    let result = if is_json {
        serde_json::from_slice::<T>(body).map_err(|e| e.to_string())
    } else {
        serde_urlencoded::from_bytes::<T>(body).map_err(|e| e.to_string())
    };

    result.map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid_request", "error_description": e})),
        )
            .into_response()
    })
}

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

/// Install a `tracing` subscriber so `TraceLayer` emits one line per request.
/// `RUST_LOG` overrides the default; set `RUST_LOG=debug` for verbose output.
fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("tower_http=info,treeline_cli=info"));

    let _ = fmt().with_env_filter(filter).try_init();
}
