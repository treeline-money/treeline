//! HTTP integration tests for the hub server's OAuth + MCP paths.
//!
//! Spins up `build_app` on 127.0.0.1:0 against a temp TREELINE_DIR
//! and exercises the endpoints with reqwest. Run with:
//!   cargo test --test serve_oauth_tests -- --nocapture

use std::sync::Arc;

use chrono::Duration;
use reqwest::StatusCode;
use tempfile::TempDir;
use tokio::net::TcpListener;

use treeline_cli::commands::serve::{build_app, AppState};
use treeline_core::adapters::duckdb::DuckDbRepository;
use treeline_core::services::hub::HubService;
use treeline_core::services::oauth::OAuthStore;

// ============================================================================
// Harness
// ============================================================================

struct TestHub {
    base_url: String,
    master_token: String,
    oauth_store: Arc<OAuthStore>,
    _temp_dir: TempDir,
    _handle: tokio::task::JoinHandle<()>,
}

async fn spawn_hub() -> TestHub {
    spawn_hub_with_ttls(Duration::days(30), Duration::days(365)).await
}

async fn spawn_hub_with_ttls(access_ttl: Duration, refresh_ttl: Duration) -> TestHub {
    spawn_hub_inner(access_ttl, refresh_ttl, true, None).await
}

/// Spawn a hub with no `treeline.duckdb` provisioned. Used to verify MCP
/// `initialize` / `tools/list` succeed before any database has been pushed.
async fn spawn_hub_without_database() -> TestHub {
    spawn_hub_inner(Duration::days(30), Duration::days(365), false, None).await
}

/// Spawn a hub with a short auth-failure window so window-expiry tests
/// don't have to wait 15 minutes.
async fn spawn_hub_with_rate_window(window: std::time::Duration) -> TestHub {
    spawn_hub_inner(Duration::days(30), Duration::days(365), true, Some(window)).await
}

async fn spawn_hub_inner(
    access_ttl: Duration,
    refresh_ttl: Duration,
    with_database: bool,
    auth_rate_window: Option<std::time::Duration>,
) -> TestHub {
    let temp_dir = TempDir::new().unwrap();
    let treeline_dir = temp_dir.path().to_path_buf();

    if with_database {
        // Provision a DuckDB so HubService::has_database() returns true.
        let db_path = treeline_dir.join("treeline.duckdb");
        let repo = DuckDbRepository::new(&db_path, None).expect("create repo");
        repo.ensure_schema().expect("schema");
        repo.checkpoint().expect("checkpoint");
    }

    // Master hub token (the "I own this hub" credential).
    let master_token = HubService::load_or_create_token(&treeline_dir).unwrap();

    let hub_service = HubService::new(treeline_dir.clone(), "treeline.duckdb".to_string());
    let oauth_store = Arc::new(OAuthStore::with_ttls(
        treeline_dir.clone(),
        access_ttl,
        refresh_ttl,
    ));

    let mut state = AppState::new(treeline_dir.clone(), hub_service, oauth_store.clone());
    if let Some(window) = auth_rate_window {
        state.auth_rate_window = window;
    }
    let state = Arc::new(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = build_app(state);

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    TestHub {
        base_url: format!("http://{}", addr),
        master_token,
        oauth_store,
        _temp_dir: temp_dir,
        _handle: handle,
    }
}

fn no_redirect_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

fn pkce_pair() -> (String, String) {
    use base64::Engine;
    use sha2::{Digest, Sha256};

    let verifier = "abc123_this_is_a_pkce_code_verifier_that_is_at_least_43_characters_long_yes";
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash);
    (verifier.to_string(), challenge)
}

/// Run the full OAuth dance over HTTP (register → authorize → token),
/// returning (access_token, refresh_token, client_id).
async fn full_oauth_flow(hub: &TestHub, scope: &str) -> (String, String, String) {
    let c = reqwest::Client::new();
    let redir = no_redirect_client();

    // /register
    let resp = c
        .post(format!("{}/register", hub.base_url))
        .json(&serde_json::json!({
            "redirect_uris": ["http://localhost/cb"],
            "client_name": "Test Client",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.unwrap();
    let client_id = body["client_id"].as_str().unwrap().to_string();

    // /authorize POST (form)
    let (verifier, challenge) = pkce_pair();
    let resp = redir
        .post(format!("{}/authorize", hub.base_url))
        .form(&[
            ("hub_token", hub.master_token.as_str()),
            ("redirect_uri", "http://localhost/cb"),
            ("state", "state_abc"),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("client_id", client_id.as_str()),
            ("scope", scope),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FOUND);
    let location = resp.headers().get("location").unwrap().to_str().unwrap();

    // Extract `code` from the redirect URL query string.
    let parsed = url::Url::parse(location).unwrap();
    let code = parsed
        .query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.into_owned())
        .expect("redirect includes code");

    // /token exchange.
    let resp = c
        .post(format!("{}/token", hub.base_url))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("code_verifier", &verifier),
            ("redirect_uri", "http://localhost/cb"),
            ("client_id", &client_id),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();

    (
        body["access_token"].as_str().unwrap().to_string(),
        body["refresh_token"].as_str().unwrap().to_string(),
        client_id,
    )
}

async fn mcp_tools_call(
    hub: &TestHub,
    token: &str,
    tool_name: &str,
    args: serde_json::Value,
) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/mcp", hub.base_url))
        .bearer_auth(token)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": tool_name, "arguments": args },
        }))
        .send()
        .await
        .unwrap()
}

// ============================================================================
// Full OAuth flow
// ============================================================================

#[tokio::test]
async fn full_flow_issues_access_token_distinct_from_master() {
    let hub = spawn_hub().await;
    let (access, refresh, _) = full_oauth_flow(&hub, "read write").await;

    assert_ne!(access, hub.master_token);
    assert_ne!(refresh, hub.master_token);
    assert_ne!(access, refresh);
    assert_eq!(access.len(), 64); // 32 bytes hex
}

#[tokio::test]
async fn register_endpoint_persists_client_across_server_views() {
    let hub = spawn_hub().await;
    let c = reqwest::Client::new();
    let resp = c
        .post(format!("{}/register", hub.base_url))
        .json(&serde_json::json!({
            "redirect_uris": ["http://localhost/cb"],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.unwrap();
    let client_id = body["client_id"].as_str().unwrap();

    // OAuthStore sees the same client_id.
    let clients = hub.oauth_store.list_clients().unwrap();
    assert!(clients.iter().any(|c| c.client_id == client_id));
}

#[tokio::test]
async fn authorize_with_wrong_hub_token_does_not_issue_code() {
    let hub = spawn_hub().await;
    let redir = no_redirect_client();
    let (_verifier, challenge) = pkce_pair();

    let resp = redir
        .post(format!("{}/authorize", hub.base_url))
        .form(&[
            ("hub_token", "not_the_hub_token"),
            ("redirect_uri", "http://localhost/cb"),
            ("state", "s"),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("scope", "read"),
        ])
        .send()
        .await
        .unwrap();

    // Should NOT redirect with a code. Current impl returns 200 HTML error page;
    // the important thing is that no redirect to the client happens.
    assert_ne!(resp.status(), StatusCode::FOUND);
}

// ============================================================================
// /mcp authentication + scope enforcement
// ============================================================================

#[tokio::test]
async fn mcp_with_per_client_token_can_call_read_tool() {
    let hub = spawn_hub().await;
    let (access, _, _) = full_oauth_flow(&hub, "read").await;

    let resp = mcp_tools_call(&hub, &access, "status", serde_json::json!({})).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.get("result").is_some(), "body was {}", body);
}

#[tokio::test]
async fn mcp_with_read_scope_rejects_write_tool() {
    let hub = spawn_hub().await;
    let (access, _, _) = full_oauth_flow(&hub, "read").await;

    let resp = mcp_tools_call(
        &hub,
        &access,
        "tag",
        serde_json::json!({"tags": "foo", "transaction_ids": []}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let www_auth = resp
        .headers()
        .get("www-authenticate")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        www_auth.contains("insufficient_scope"),
        "expected WWW-Authenticate: insufficient_scope, got {:?}",
        www_auth
    );
}

#[tokio::test]
async fn mcp_with_write_scope_allows_write_tool() {
    let hub = spawn_hub().await;
    let (access, _, _) = full_oauth_flow(&hub, "read write").await;

    // `tag` on a made-up uuid — expect the tool to run (may return app-level
    // error inside the JSON-RPC result, but the HTTP status should be 200).
    let resp = mcp_tools_call(
        &hub,
        &access,
        "tag",
        serde_json::json!({
            "tags": "test",
            "transaction_ids": ["00000000-0000-0000-0000-000000000000"],
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn mcp_with_master_hub_token_is_rejected() {
    // No backward compat: /mcp must use OAuth-issued tokens.
    let hub = spawn_hub().await;
    let resp = mcp_tools_call(&hub, &hub.master_token, "status", serde_json::json!({})).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn mcp_with_no_token_is_rejected() {
    let hub = spawn_hub().await;
    let resp = reqwest::Client::new()
        .post(format!("{}/mcp", hub.base_url))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn mcp_with_revoked_token_is_rejected() {
    let hub = spawn_hub().await;
    let (access, _, _) = full_oauth_flow(&hub, "read").await;

    // Directly revoke in the store (revocation endpoint covered in its own test).
    hub.oauth_store.revoke_access_token(&access).unwrap();

    let resp = mcp_tools_call(&hub, &access, "status", serde_json::json!({})).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn mcp_with_expired_access_token_is_rejected() {
    let hub = spawn_hub_with_ttls(Duration::seconds(-1), Duration::days(30)).await;
    let (access, _, _) = full_oauth_flow(&hub, "read").await;

    let resp = mcp_tools_call(&hub, &access, "status", serde_json::json!({})).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ============================================================================
// /revoke
// ============================================================================

#[tokio::test]
async fn revoke_endpoint_invalidates_access_token() {
    let hub = spawn_hub().await;
    let (access, _, _) = full_oauth_flow(&hub, "read").await;

    // Call /revoke per RFC 7009: form-encoded with `token`.
    let resp = reqwest::Client::new()
        .post(format!("{}/revoke", hub.base_url))
        .form(&[("token", access.as_str())])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Subsequent /mcp call fails.
    let resp = mcp_tools_call(&hub, &access, "status", serde_json::json!({})).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn revoke_refresh_cascades_to_access_token() {
    let hub = spawn_hub().await;
    let (access, refresh, _) = full_oauth_flow(&hub, "read").await;

    let resp = reqwest::Client::new()
        .post(format!("{}/revoke", hub.base_url))
        .form(&[
            ("token", refresh.as_str()),
            ("token_type_hint", "refresh_token"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Access token also dead.
    let resp = mcp_tools_call(&hub, &access, "status", serde_json::json!({})).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ============================================================================
// Refresh grant
// ============================================================================

#[tokio::test]
async fn refresh_grant_issues_new_access_token() {
    let hub = spawn_hub().await;
    let (_initial_access, refresh, _) = full_oauth_flow(&hub, "read").await;

    let resp = reqwest::Client::new()
        .post(format!("{}/token", hub.base_url))
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let new_access = body["access_token"].as_str().unwrap();
    assert!(!new_access.is_empty());

    // New access token works.
    let resp = mcp_tools_call(&hub, new_access, "status", serde_json::json!({})).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

// ============================================================================
// /api/push — still master-token-only
// ============================================================================

#[tokio::test]
async fn api_push_with_no_auth_is_rejected() {
    let hub = spawn_hub().await;
    let resp = reqwest::Client::new()
        .post(format!("{}/api/push", hub.base_url))
        .body(b"garbage".to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn api_push_with_mcp_token_is_rejected_for_lacking_push_scope() {
    // An MCP-scoped token (read/write) must NOT unlock /api/push — that
    // gate is `push`. Returns 403 with insufficient_scope, not 401.
    let hub = spawn_hub().await;
    let (access, _, _) = full_oauth_flow(&hub, "read write").await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/push", hub.base_url))
        .bearer_auth(&access)
        .body(b"garbage".to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let www_auth = resp
        .headers()
        .get("www-authenticate")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        www_auth.contains("insufficient_scope"),
        "expected insufficient_scope, got {:?}",
        www_auth
    );
}

#[tokio::test]
async fn api_push_with_master_token_is_now_rejected() {
    // Master no longer unlocks /api/* — devices must link via OAuth to
    // get a push-scoped token.
    let hub = spawn_hub().await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/push", hub.base_url))
        .bearer_auth(&hub.master_token)
        .body(b"garbage".to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ============================================================================
// OAuth metadata — unchanged, but smoke test
// ============================================================================

#[tokio::test]
async fn oauth_metadata_endpoints_are_reachable() {
    let hub = spawn_hub().await;
    let resp = reqwest::Client::new()
        .get(format!(
            "{}/.well-known/oauth-authorization-server",
            hub.base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["authorization_endpoint"].is_string());
    assert!(body["token_endpoint"].is_string());
    assert!(body["registration_endpoint"].is_string());
}

// ============================================================================
// /mcp behavior on a freshly-provisioned hub with no database yet
//
// Thin clients (Claude.ai, ChatGPT, Claude Desktop) need to complete OAuth and
// `initialize` / `tools/list` *before* the user has pushed any data — the hub
// is effectively a Fly app with just OAuth state on disk. These tests pin
// down: protocol-level handshakes succeed, data-requiring tool calls fail
// gracefully via JSON-RPC error semantics (HTTP 200 + isError result), and
// non-DB tools (e.g. `version`) keep working.
// ============================================================================

async fn mcp_post(hub: &TestHub, token: &str, body: serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/mcp", hub.base_url))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn mcp_initialize_succeeds_without_database() {
    let hub = spawn_hub_without_database().await;
    let (access, _, _) = full_oauth_flow(&hub, "read").await;

    let resp = mcp_post(
        &hub,
        &access,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.get("result").is_some(), "body was {}", body);
    assert_eq!(body["result"]["serverInfo"]["name"], "treeline");
    assert!(body["result"]["protocolVersion"].is_string());
}

#[tokio::test]
async fn mcp_tools_list_succeeds_without_database() {
    let hub = spawn_hub_without_database().await;
    let (access, _, _) = full_oauth_flow(&hub, "read").await;

    let resp = mcp_post(
        &hub,
        &access,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
        }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let tools = body["result"]["tools"].as_array().expect("tools array");
    assert!(!tools.is_empty(), "tools list should not be empty");
    // Sanity: the schema-discovery tools the user will call first are present.
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"query"));
    assert!(names.contains(&"schema"));
    assert!(names.contains(&"version"));
}

#[tokio::test]
async fn mcp_data_tool_call_without_database_returns_jsonrpc_error_at_http_200() {
    let hub = spawn_hub_without_database().await;
    let (access, _, _) = full_oauth_flow(&hub, "read").await;

    // `query` is in TOOLS_REQUIRING_DB. Without a DB on disk, the hub must
    // return HTTP 200 + a JSON-RPC success envelope carrying isError=true,
    // *not* HTTP 400 (Claude treats non-200 as a transport failure and never
    // surfaces the error message to the user).
    let resp = mcp_tools_call(
        &hub,
        &access,
        "query",
        serde_json::json!({"sql": "SELECT 1"}),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["jsonrpc"], "2.0");
    assert_eq!(body["result"]["isError"], true);
    let text = body["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(
        text.to_lowercase().contains("no database"),
        "expected helpful empty-state message, got: {}",
        text
    );
}

#[tokio::test]
async fn mcp_version_tool_works_without_database() {
    // Tools that don't read DuckDB (`version`, `encryption_status`, `skills_*`,
    // `demo`) must keep working before any push. Otherwise the dashboard's
    // "Connect AI assistants" link is dead until the user does a CLI push.
    let hub = spawn_hub_without_database().await;
    let (access, _, _) = full_oauth_flow(&hub, "read").await;

    let resp = mcp_tools_call(&hub, &access, "version", serde_json::json!({})).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["result"].get("isError").is_none() || body["result"]["isError"] == false);
    let text = body["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(
        text.contains("current_version"),
        "expected version payload, got: {}",
        text
    );
}

#[tokio::test]
async fn mcp_malformed_json_returns_parse_error_at_http_200() {
    // JSON-RPC parse errors must also travel at HTTP 200 — same reasoning as
    // the data-tool case above.
    let hub = spawn_hub_without_database().await;
    let (access, _, _) = full_oauth_flow(&hub, "read").await;

    let resp = reqwest::Client::new()
        .post(format!("{}/mcp", hub.base_url))
        .bearer_auth(&access)
        .header("content-type", "application/json")
        .body("{ this is not json")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], -32700);
}

// ============================================================================
// Device-code flow (RFC 8628)
//
// End-to-end coverage of `tl hub link`'s server side: CLI POSTs /device/code,
// browser-side POSTs /authorize with master + user_code, CLI poll on /token
// returns tokens. Plus the negative cases: pending, expired, master required.
// ============================================================================

/// Drive the device-code dance against `hub`, simulating what `tl hub link`
/// does. Returns (access_token, refresh_token, scopes).
async fn full_device_code_flow(hub: &TestHub, scope: &str) -> (String, String, Vec<String>) {
    let c = reqwest::Client::new();

    // Step 1: CLI initiates.
    let resp = c
        .post(format!("{}/device/code", hub.base_url))
        .form(&[("scope", scope), ("client_name", "test-laptop")])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let device_code = body["device_code"].as_str().unwrap().to_string();
    let user_code = body["user_code"].as_str().unwrap().to_string();
    assert!(body["verification_uri_complete"]
        .as_str()
        .unwrap()
        .contains(&user_code));

    // Step 2: simulate the browser-side authorize (user types master + clicks).
    let resp = c
        .post(format!("{}/authorize", hub.base_url))
        .form(&[
            ("user_code", user_code.as_str()),
            ("hub_token", hub.master_token.as_str()),
            ("client_name", "Zack's Laptop"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Step 3: CLI poll succeeds.
    let resp = c
        .post(format!("{}/token", hub.base_url))
        .form(&[
            ("grant_type", "device_code"),
            ("device_code", device_code.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let access = body["access_token"].as_str().unwrap().to_string();
    let refresh = body["refresh_token"].as_str().unwrap().to_string();
    let scopes: Vec<String> = body["scope"]
        .as_str()
        .unwrap_or("")
        .split_whitespace()
        .map(String::from)
        .collect();
    (access, refresh, scopes)
}

#[tokio::test]
async fn device_code_flow_issues_pull_push_scoped_token() {
    let hub = spawn_hub().await;
    let (access, refresh, scopes) = full_device_code_flow(&hub, "pull push").await;

    assert!(scopes.contains(&"pull".to_string()));
    assert!(scopes.contains(&"push".to_string()));
    assert_ne!(access, refresh);
    assert_ne!(access, hub.master_token);
}

#[tokio::test]
async fn device_code_poll_returns_authorization_pending_before_user_authorizes() {
    let hub = spawn_hub().await;
    let c = reqwest::Client::new();

    let resp = c
        .post(format!("{}/device/code", hub.base_url))
        .form(&[("scope", "pull push"), ("client_name", "test")])
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let device_code = body["device_code"].as_str().unwrap();

    // Poll immediately — no one authorized yet.
    let resp = c
        .post(format!("{}/token", hub.base_url))
        .form(&[("grant_type", "device_code"), ("device_code", device_code)])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "authorization_pending");
}

#[tokio::test]
async fn device_code_poll_returns_invalid_grant_for_unknown_code() {
    let hub = spawn_hub().await;
    let resp = reqwest::Client::new()
        .post(format!("{}/token", hub.base_url))
        .form(&[
            ("grant_type", "device_code"),
            ("device_code", "deadbeef-not-a-real-device-code"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "invalid_grant");
}

// ============================================================================
// /authorize JSON content negotiation
//
// Pro orchestrates the device-code flow server-side; HTML responses with
// distinctive headings made the adapter brittle to UI tweaks. With
// `Accept: application/json` the handler returns structured JSON and
// proper status codes instead.
// ============================================================================

#[tokio::test]
async fn authorize_returns_json_when_accept_header_says_so() {
    let hub = spawn_hub().await;
    let c = reqwest::Client::new();

    // Start a device-code session.
    let resp = c
        .post(format!("{}/device/code", hub.base_url))
        .form(&[("scope", "pull push"), ("client_name", "x")])
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let user_code = body["user_code"].as_str().unwrap().to_string();

    // Authorize with Accept: application/json.
    let resp = c
        .post(format!("{}/authorize", hub.base_url))
        .header("Accept", "application/json")
        .form(&[
            ("user_code", user_code.as_str()),
            ("hub_token", hub.master_token.as_str()),
            ("client_name", "Json Caller"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .starts_with("application/json"));
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "linked");
}

#[tokio::test]
async fn authorize_json_returns_401_for_invalid_master() {
    let hub = spawn_hub().await;
    let c = reqwest::Client::new();

    let resp = c
        .post(format!("{}/device/code", hub.base_url))
        .form(&[("scope", "pull push"), ("client_name", "x")])
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let user_code = body["user_code"].as_str().unwrap().to_string();

    let resp = c
        .post(format!("{}/authorize", hub.base_url))
        .header("Accept", "application/json")
        .form(&[
            ("user_code", user_code.as_str()),
            ("hub_token", "wrong-master"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "invalid_token");
}

#[tokio::test]
async fn authorize_json_returns_404_for_unknown_user_code() {
    let hub = spawn_hub().await;
    let resp = reqwest::Client::new()
        .post(format!("{}/authorize", hub.base_url))
        .header("Accept", "application/json")
        .form(&[
            ("user_code", "NOPE-NOPE"),
            ("hub_token", hub.master_token.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "code_not_found");
}

#[tokio::test]
async fn authorize_returns_html_by_default_browser_path_unchanged() {
    let hub = spawn_hub().await;
    let c = reqwest::Client::new();

    let resp = c
        .post(format!("{}/device/code", hub.base_url))
        .form(&[("scope", "pull push"), ("client_name", "x")])
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let user_code = body["user_code"].as_str().unwrap().to_string();

    // No Accept header → still returns HTML (browser-friendly fallback).
    let resp = c
        .post(format!("{}/authorize", hub.base_url))
        .form(&[
            ("user_code", user_code.as_str()),
            ("hub_token", hub.master_token.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.text().await.unwrap();
    assert!(body.contains("Device linked"));
    assert!(body.contains("<html"));
}

#[tokio::test]
async fn device_code_authorize_with_wrong_master_does_not_complete() {
    let hub = spawn_hub().await;
    let c = reqwest::Client::new();

    let resp = c
        .post(format!("{}/device/code", hub.base_url))
        .form(&[("scope", "pull push"), ("client_name", "test")])
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let device_code = body["device_code"].as_str().unwrap().to_string();
    let user_code = body["user_code"].as_str().unwrap().to_string();

    // Submit /authorize with a wrong master.
    let _ = c
        .post(format!("{}/authorize", hub.base_url))
        .form(&[
            ("user_code", user_code.as_str()),
            ("hub_token", "not-the-master"),
        ])
        .send()
        .await
        .unwrap();

    // Session still pending — authorize page returns an error page (not a
    // redirect), but the session wasn't transitioned.
    let resp = c
        .post(format!("{}/token", hub.base_url))
        .form(&[
            ("grant_type", "device_code"),
            ("device_code", device_code.as_str()),
        ])
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "authorization_pending");
}

#[tokio::test]
async fn device_code_user_code_normalization_is_dash_and_case_insensitive() {
    // User might type `gztm-xkqr` or `GZTMXKQR` instead of `GZTM-XKQR`.
    // The /authorize page should normalize before lookup.
    let hub = spawn_hub().await;
    let c = reqwest::Client::new();

    let resp = c
        .post(format!("{}/device/code", hub.base_url))
        .form(&[("scope", "pull push"), ("client_name", "t")])
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let device_code = body["device_code"].as_str().unwrap().to_string();
    let user_code = body["user_code"].as_str().unwrap().to_string();

    // Strip dash, lowercase, submit — should still resolve.
    let mangled = user_code.replace('-', "").to_lowercase();

    let resp = c
        .post(format!("{}/authorize", hub.base_url))
        .form(&[
            ("user_code", mangled.as_str()),
            ("hub_token", hub.master_token.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Poll should now succeed.
    let resp = c
        .post(format!("{}/token", hub.base_url))
        .form(&[
            ("grant_type", "device_code"),
            ("device_code", device_code.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// ============================================================================
// Pull/Push scope enforcement on /api/*
// ============================================================================

#[tokio::test]
async fn api_pull_with_pull_scoped_token_is_authorized() {
    let hub = spawn_hub().await;
    let (access, _, _) = full_device_code_flow(&hub, "pull push").await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/pull", hub.base_url))
        .bearer_auth(&access)
        .send()
        .await
        .unwrap();
    // Auth succeeded — body might be a bundle (200) or a server error if
    // the test DB lacks something; we just want to know it isn't 401/403.
    assert_ne!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_ne!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn api_push_with_push_scoped_token_passes_auth() {
    let hub = spawn_hub().await;
    let (access, _, _) = full_device_code_flow(&hub, "pull push").await;

    // Garbage body — auth succeeds, body parsing fails (400). The point
    // is we got past auth.
    let resp = reqwest::Client::new()
        .post(format!("{}/api/push", hub.base_url))
        .bearer_auth(&access)
        .body(b"garbage".to_vec())
        .send()
        .await
        .unwrap();
    assert_ne!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_ne!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn api_pull_with_pull_only_scope_works() {
    let hub = spawn_hub().await;
    let (access, _, _) = full_device_code_flow(&hub, "pull").await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/pull", hub.base_url))
        .bearer_auth(&access)
        .send()
        .await
        .unwrap();
    assert_ne!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_ne!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn api_push_with_pull_only_scope_is_rejected() {
    let hub = spawn_hub().await;
    let (access, _, _) = full_device_code_flow(&hub, "pull").await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/push", hub.base_url))
        .bearer_auth(&access)
        .body(b"garbage".to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn api_hash_with_pull_scope_is_authorized() {
    let hub = spawn_hub().await;
    let (access, _, _) = full_device_code_flow(&hub, "pull push").await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/hash", hub.base_url))
        .bearer_auth(&access)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// ============================================================================
// /api/clients listing
// ============================================================================

#[tokio::test]
async fn api_clients_lists_devices_and_apps_with_kind() {
    let hub = spawn_hub().await;
    // One device, one MCP app.
    let _ = full_device_code_flow(&hub, "pull push").await;
    let _ = full_oauth_flow(&hub, "read write").await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/clients", hub.base_url))
        .bearer_auth(&hub.master_token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let clients = body["clients"].as_array().expect("clients array");
    assert_eq!(
        clients.len(),
        2,
        "expected 1 device + 1 app, got {:?}",
        clients
    );

    let kinds: Vec<&str> = clients.iter().filter_map(|c| c["kind"].as_str()).collect();
    assert!(kinds.contains(&"device"), "kinds: {:?}", kinds);
    assert!(kinds.contains(&"app"), "kinds: {:?}", kinds);
}

#[tokio::test]
async fn api_clients_requires_master_token() {
    let hub = spawn_hub().await;
    let (access, _, _) = full_device_code_flow(&hub, "pull push").await;

    // Device-scoped token must NOT unlock /api/clients (admin-only).
    let resp = reqwest::Client::new()
        .get(format!("{}/api/clients", hub.base_url))
        .bearer_auth(&access)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // No auth at all → 401.
    let resp = reqwest::Client::new()
        .get(format!("{}/api/clients", hub.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn api_clients_carries_friendly_name_and_scopes() {
    let hub = spawn_hub().await;
    let _ = full_device_code_flow(&hub, "pull push").await;

    let resp = reqwest::Client::new()
        .get(format!("{}/api/clients", hub.base_url))
        .bearer_auth(&hub.master_token)
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let clients = body["clients"].as_array().unwrap();
    assert_eq!(clients.len(), 1);
    let entry = &clients[0];

    // Name comes from /authorize's "client_name" field, which the device-
    // code flow sets to "Zack's Laptop".
    assert_eq!(entry["name"], "Zack's Laptop");
    let scopes = entry["scopes"].as_array().unwrap();
    let scope_strs: Vec<&str> = scopes.iter().filter_map(|s| s.as_str()).collect();
    assert!(scope_strs.contains(&"pull"));
    assert!(scope_strs.contains(&"push"));
    assert_eq!(entry["kind"], "device");
}

// ============================================================================
// DELETE /api/clients/{client_id} — revoke
// ============================================================================

#[tokio::test]
async fn revoke_client_wipes_tokens_and_drops_from_listing() {
    let hub = spawn_hub().await;
    let _ = full_device_code_flow(&hub, "pull push").await;

    // Find the issued client_id via the listing.
    let resp = reqwest::Client::new()
        .get(format!("{}/api/clients", hub.base_url))
        .bearer_auth(&hub.master_token)
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let client_id = body["clients"][0]["client_id"]
        .as_str()
        .expect("client_id present")
        .to_string();

    // Revoke.
    let resp = reqwest::Client::new()
        .delete(format!("{}/api/clients/{}", hub.base_url, client_id))
        .bearer_auth(&hub.master_token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Now listing should be empty (no live tokens for that client).
    let resp = reqwest::Client::new()
        .get(format!("{}/api/clients", hub.base_url))
        .bearer_auth(&hub.master_token)
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let clients = body["clients"].as_array().unwrap();
    assert!(
        clients.is_empty(),
        "expected empty listing after revoke, got {:?}",
        clients
    );
}

#[tokio::test]
async fn revoke_client_requires_master_token() {
    let hub = spawn_hub().await;
    let (access, _, _) = full_device_code_flow(&hub, "pull push").await;

    // Device-scoped token must not unlock revoke.
    let resp = reqwest::Client::new()
        .delete(format!("{}/api/clients/anything", hub.base_url))
        .bearer_auth(&access)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // No auth at all → 401.
    let resp = reqwest::Client::new()
        .delete(format!("{}/api/clients/anything", hub.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ============================================================================
// Per-IP rate limiting on credential endpoints
//
// 5 failed credential attempts per IP per window → 429 on /authorize,
// /token, and the master-gated /api/clients routes. Successes clear the
// counter; device-code pending polls never count; other IPs are unaffected.
// Tests spoof distinct attackers via Fly-Client-IP (what the server trusts
// behind Fly's proxy).
// ============================================================================

/// One failed hub_token guess against POST /authorize from `ip`.
async fn bad_authorize_attempt(hub: &TestHub, ip: &str) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/authorize", hub.base_url))
        .header("Fly-Client-IP", ip)
        .form(&[
            ("hub_token", "not-the-hub-token"),
            ("redirect_uri", "http://localhost/cb"),
            ("state", "s"),
            ("code_challenge", "c"),
            ("code_challenge_method", "S256"),
            ("scope", "read"),
        ])
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn sixth_failed_authorize_attempt_is_blocked() {
    let hub = spawn_hub().await;
    for i in 1..=5 {
        let resp = bad_authorize_attempt(&hub, "203.0.113.1").await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "attempt {} should get the error page, not a lockout",
            i
        );
    }
    let resp = bad_authorize_attempt(&hub, "203.0.113.1").await;
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    let body = resp.text().await.unwrap();
    assert!(body.contains("Too many attempts"), "body was {}", body);
}

#[tokio::test]
async fn lockout_is_scoped_to_the_failing_ip() {
    let hub = spawn_hub().await;
    let c = reqwest::Client::new();

    // Lock out one IP.
    for _ in 0..6 {
        bad_authorize_attempt(&hub, "203.0.113.2").await;
    }
    let resp = bad_authorize_attempt(&hub, "203.0.113.2").await;
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

    // A different IP completes a real device-code link during the lockout.
    let resp = c
        .post(format!("{}/device/code", hub.base_url))
        .form(&[("scope", "pull push"), ("client_name", "other-laptop")])
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let device_code = body["device_code"].as_str().unwrap().to_string();
    let user_code = body["user_code"].as_str().unwrap().to_string();

    let resp = c
        .post(format!("{}/authorize", hub.base_url))
        .header("Fly-Client-IP", "203.0.113.3")
        .form(&[
            ("user_code", user_code.as_str()),
            ("hub_token", hub.master_token.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = c
        .post(format!("{}/token", hub.base_url))
        .form(&[
            ("grant_type", "device_code"),
            ("device_code", device_code.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn successful_auth_clears_failure_counter() {
    let hub = spawn_hub().await;
    let c = reqwest::Client::new();
    let ip = "203.0.113.4";

    // 4 failures, then a success (device-code completion), then 4 more
    // failures. Without the clear-on-success, that's 8 accumulated failures
    // and the final valid attempt would be locked out.
    for _ in 0..4 {
        bad_authorize_attempt(&hub, ip).await;
    }

    let start_device_session = || async {
        let resp = c
            .post(format!("{}/device/code", hub.base_url))
            .form(&[("scope", "pull push"), ("client_name", "laptop")])
            .send()
            .await
            .unwrap();
        let body: serde_json::Value = resp.json().await.unwrap();
        body["user_code"].as_str().unwrap().to_string()
    };

    let user_code = start_device_session().await;
    let resp = c
        .post(format!("{}/authorize", hub.base_url))
        .header("Fly-Client-IP", ip)
        .form(&[
            ("user_code", user_code.as_str()),
            ("hub_token", hub.master_token.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    for _ in 0..4 {
        bad_authorize_attempt(&hub, ip).await;
    }

    let user_code = start_device_session().await;
    let resp = c
        .post(format!("{}/authorize", hub.base_url))
        .header("Fly-Client-IP", ip)
        .form(&[
            ("user_code", user_code.as_str()),
            ("hub_token", hub.master_token.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "counter should have reset on the earlier success"
    );
}

#[tokio::test]
async fn window_expiry_unblocks_a_locked_out_ip() {
    // The window must be wide enough that a slow CI runner can't take
    // longer than it to make the 5 attempts — otherwise the window expires
    // mid-loop, the counter resets, and the 6th attempt isn't blocked.
    let hub = spawn_hub_with_rate_window(std::time::Duration::from_secs(3)).await;
    let ip = "203.0.113.5";

    for _ in 0..5 {
        bad_authorize_attempt(&hub, ip).await;
    }
    let resp = bad_authorize_attempt(&hub, ip).await;
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

    tokio::time::sleep(std::time::Duration::from_millis(3500)).await;

    // Window expired — attempts flow again (and still fail normally).
    let resp = bad_authorize_attempt(&hub, ip).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn device_code_completion_failures_count_toward_lockout() {
    let hub = spawn_hub().await;
    let c = reqwest::Client::new();
    let ip = "203.0.113.6";

    let resp = c
        .post(format!("{}/device/code", hub.base_url))
        .form(&[("scope", "pull push"), ("client_name", "laptop")])
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let user_code = body["user_code"].as_str().unwrap().to_string();

    // 5 wrong-master submits on the device completion path, then the 6th —
    // even with the *correct* master — is locked out.
    for _ in 0..5 {
        let resp = c
            .post(format!("{}/authorize", hub.base_url))
            .header("Fly-Client-IP", ip)
            .form(&[
                ("user_code", user_code.as_str()),
                ("hub_token", "not-the-master"),
            ])
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
    let resp = c
        .post(format!("{}/authorize", hub.base_url))
        .header("Fly-Client-IP", ip)
        .form(&[
            ("user_code", user_code.as_str()),
            ("hub_token", hub.master_token.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn token_endpoint_blocks_after_repeated_invalid_grants() {
    let hub = spawn_hub().await;
    let c = reqwest::Client::new();
    let ip = "203.0.113.7";

    for i in 1..=5 {
        let resp = c
            .post(format!("{}/token", hub.base_url))
            .header("Fly-Client-IP", ip)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", "guessed-refresh-token"),
            ])
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "attempt {}", i);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["error"], "invalid_grant");
    }

    let resp = c
        .post(format!("{}/token", hub.base_url))
        .header("Fly-Client-IP", ip)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", "guessed-refresh-token"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "slow_down");
}

#[tokio::test]
async fn device_code_pending_polls_do_not_count_as_failures() {
    let hub = spawn_hub().await;
    let c = reqwest::Client::new();

    let resp = c
        .post(format!("{}/device/code", hub.base_url))
        .form(&[("scope", "pull push"), ("client_name", "laptop")])
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let device_code = body["device_code"].as_str().unwrap().to_string();
    let user_code = body["user_code"].as_str().unwrap().to_string();

    // A real CLI polls many times while waiting for the browser step. All
    // requests here come from the same (unspoofed → "local") IP.
    for _ in 0..10 {
        let resp = c
            .post(format!("{}/token", hub.base_url))
            .form(&[
                ("grant_type", "device_code"),
                ("device_code", device_code.as_str()),
            ])
            .send()
            .await
            .unwrap();
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["error"], "authorization_pending");
    }

    // The same IP can still complete the flow — polling didn't lock it out.
    let resp = c
        .post(format!("{}/authorize", hub.base_url))
        .form(&[
            ("user_code", user_code.as_str()),
            ("hub_token", hub.master_token.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = c
        .post(format!("{}/token", hub.base_url))
        .form(&[
            ("grant_type", "device_code"),
            ("device_code", device_code.as_str()),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn api_clients_master_guesses_count_toward_lockout() {
    let hub = spawn_hub().await;
    let c = reqwest::Client::new();
    let ip = "203.0.113.8";

    for _ in 0..5 {
        let resp = c
            .get(format!("{}/api/clients", hub.base_url))
            .header("Fly-Client-IP", ip)
            .bearer_auth("guessed-master-token")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    let resp = c
        .get(format!("{}/api/clients", hub.base_url))
        .header("Fly-Client-IP", ip)
        .bearer_auth("guessed-master-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

    // The real master from another IP is unaffected.
    let resp = c
        .get(format!("{}/api/clients", hub.base_url))
        .header("Fly-Client-IP", "203.0.113.9")
        .bearer_auth(&hub.master_token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn revoke_unknown_client_is_idempotent() {
    let hub = spawn_hub().await;

    let resp = reqwest::Client::new()
        .delete(format!("{}/api/clients/does-not-exist", hub.base_url))
        .bearer_auth(&hub.master_token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}
