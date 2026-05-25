//! OAuth store — per-client access and refresh tokens.
//!
//! The hub's OAuth layer issues scoped tokens to individual thin clients
//! (Claude Desktop, claude.ai, etc.) so that each client's access can be
//! revoked, scoped, and audited independently of the master hub token.
//!
//! State lives in `oauth.json` alongside `hub-sync.json`. It is NOT
//! included in the sync bundle — tokens are hub-local.
//!
//! Token TTLs are configurable via env vars:
//!   TL_HUB_ACCESS_TOKEN_TTL_DAYS   (default: 30)
//!   TL_HUB_REFRESH_TOKEN_TTL_DAYS  (default: 365)

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const STATE_FILE: &str = "oauth.json";

const DEFAULT_ACCESS_TTL_DAYS: i64 = 30;
const DEFAULT_REFRESH_TTL_DAYS: i64 = 365;

// ============================================================================
// Public API types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthClient {
    pub client_id: String,
    pub client_name: Option<String>,
    pub redirect_uris: Vec<String>,
    pub registered_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ValidatedToken {
    pub client_id: String,
    pub client_name: Option<String>,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TokenSummary {
    pub access_token_prefix: String,
    pub client_id: String,
    pub client_name: Option<String>,
    pub scopes: Vec<String>,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// `Device` for a Treeline desktop / CLI (pull / push scopes), `App` for an
/// MCP client (read / write scopes). Derived from the token's scopes so the
/// caller doesn't need to know about scope strings.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientKind {
    Device,
    App,
    Other,
}

impl ClientKind {
    fn from_scopes(scopes: &[String]) -> Self {
        let has_replicate = scopes.iter().any(|s| s == "pull" || s == "push");
        let has_mcp = scopes.iter().any(|s| s == "read" || s == "write");
        match (has_replicate, has_mcp) {
            (true, false) => ClientKind::Device,
            (false, true) => ClientKind::App,
            _ => ClientKind::Other,
        }
    }
}

/// What `/api/clients` returns per row — a registered OAuth client that
/// currently has at least one valid access token. Surfaces enough for Pro
/// to render the "Devices" and "Apps" tables on the dashboard.
#[derive(Debug, Clone, Serialize)]
pub struct ClientSummary {
    pub client_id: String,
    pub name: Option<String>,
    pub kind: ClientKind,
    pub scopes: Vec<String>,
    pub created_at: DateTime<Utc>,
    /// "Last seen" for UI — the issued_at of the freshest active token.
    pub last_token_issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Error)]
pub enum ValidateError {
    #[error("access token is not recognized")]
    Unknown,
    #[error("access token has expired")]
    Expired,
}

#[derive(Debug, Error)]
pub enum ExchangeError {
    #[error("unknown or already-consumed authorization code")]
    UnknownCode,
    #[error("PKCE verification failed")]
    PkceFailed,
    #[error(
        "code_verifier is required when the authorization code was issued with a code_challenge"
    )]
    MissingVerifier,
    #[error("authorization code has expired")]
    Expired,
    #[error("i/o error: {0}")]
    Io(String),
}

#[derive(Debug, Error)]
pub enum RefreshError {
    #[error("refresh token is not recognized")]
    Unknown,
    #[error("refresh token has expired")]
    Expired,
    #[error("i/o error: {0}")]
    Io(String),
}

/// Outcome of a `/token` poll under the device-code grant. The CLI keeps
/// polling until it gets `Ok(TokenPair)` or a terminal error.
#[derive(Debug, Error)]
pub enum DeviceCodeError {
    /// User hasn't completed the browser authorization yet — keep polling.
    #[error("authorization_pending")]
    AuthorizationPending,
    /// The device_code is unknown (never issued, or already consumed).
    #[error("unknown device code")]
    Unknown,
    /// The device_code expired before the user completed authorization.
    #[error("device code expired")]
    Expired,
    /// User explicitly rejected the request.
    #[error("access denied by user")]
    AccessDenied,
    #[error("i/o error: {0}")]
    Io(String),
}

/// Returned from `start_device_authorization` — what the CLI needs to print
/// to the user (so they can open the URL in a browser).
#[derive(Debug, Clone)]
pub struct DeviceAuthorization {
    pub device_code: String,
    pub user_code: String,
    pub expires_in: i64,
    /// Suggested polling interval in seconds.
    pub interval: i64,
}

/// What the `/authorize` page renders for a device-code request — the bits
/// of the session the page needs to display to the user.
#[derive(Debug, Clone)]
pub struct DeviceSessionInfo {
    pub user_code: String,
    pub client_id: String,
    pub client_name: Option<String>,
    pub scopes: Vec<String>,
}

// ============================================================================
// On-disk records
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuthorizationCodeRecord {
    code: String,
    client_id: String,
    redirect_uri: String,
    code_challenge: Option<String>,
    scopes: Vec<String>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AccessTokenRecord {
    token: String,
    client_id: String,
    refresh_token: String,
    scopes: Vec<String>,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RefreshTokenRecord {
    token: String,
    client_id: String,
    scopes: Vec<String>,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

/// A pending or completed device-code session (RFC 8628). Created when a CLI
/// hits `POST /device/code`; resolved when the user authorizes via the
/// browser and the CLI polls `POST /token`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum DeviceSessionStatus {
    /// Waiting for the user to authorize in the browser.
    Pending,
    /// User authorized — these tokens are ready to be handed back on the
    /// next `/token` poll, then the session is removed.
    Authorized {
        access_token: String,
        refresh_token: String,
        scopes: Vec<String>,
    },
    /// User explicitly clicked "deny" (not currently surfaced — placeholder).
    Denied,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeviceSessionRecord {
    device_code: String,
    user_code: String,
    client_id: String,
    /// Scopes the CLI requested. The browser-side authorize step will mint
    /// tokens with exactly these scopes (no widening).
    requested_scopes: Vec<String>,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    #[serde(flatten)]
    status: DeviceSessionStatus,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct OAuthState {
    #[serde(default)]
    clients: Vec<OAuthClient>,
    #[serde(default)]
    authorization_codes: Vec<AuthorizationCodeRecord>,
    #[serde(default)]
    access_tokens: Vec<AccessTokenRecord>,
    #[serde(default)]
    refresh_tokens: Vec<RefreshTokenRecord>,
    #[serde(default)]
    device_sessions: Vec<DeviceSessionRecord>,
}

// Authorization codes are short-lived — drop after this long.
const AUTH_CODE_LIFETIME_SECS: i64 = 600;

// Device-code sessions live a bit longer (10 min) — user has to context-switch
// to a browser and complete login, which can take a beat.
const DEVICE_CODE_LIFETIME_SECS: i64 = 600;
const DEVICE_CODE_POLL_INTERVAL_SECS: i64 = 2;

// ============================================================================
// Store
// ============================================================================

pub struct OAuthStore {
    treeline_dir: PathBuf,
    access_ttl: Duration,
    refresh_ttl: Duration,
    // Serializes file read-modify-write for the state file.
    lock: Mutex<()>,
}

impl OAuthStore {
    /// Construct with TTLs read from environment variables (or defaults).
    pub fn new(treeline_dir: PathBuf) -> Self {
        Self::with_ttls(treeline_dir, access_ttl_from_env(), refresh_ttl_from_env())
    }

    /// Construct with explicit TTLs — primarily for tests.
    pub fn with_ttls(treeline_dir: PathBuf, access_ttl: Duration, refresh_ttl: Duration) -> Self {
        Self {
            treeline_dir,
            access_ttl,
            refresh_ttl,
            lock: Mutex::new(()),
        }
    }

    // ------------------------------------------------------------------------
    // Clients
    // ------------------------------------------------------------------------

    pub fn register_client(
        &self,
        redirect_uris: Vec<String>,
        client_name: Option<String>,
    ) -> Result<OAuthClient> {
        let client = OAuthClient {
            client_id: generate_token(),
            client_name,
            redirect_uris,
            registered_at: Utc::now(),
        };

        let _guard = self.lock.lock().unwrap();
        let mut state = self.load_state()?;
        state.clients.push(client.clone());
        self.save_state(&state)?;

        Ok(client)
    }

    pub fn list_clients(&self) -> Result<Vec<OAuthClient>> {
        let _guard = self.lock.lock().unwrap();
        Ok(self.load_state()?.clients)
    }

    pub fn get_client(&self, client_id: &str) -> Result<Option<OAuthClient>> {
        let _guard = self.lock.lock().unwrap();
        Ok(self
            .load_state()?
            .clients
            .into_iter()
            .find(|c| c.client_id == client_id))
    }

    /// Update a client's friendly name. Used during device-code completion
    /// when the user types a name into the authorize page that's different
    /// from what the CLI registered with.
    pub fn set_client_name(&self, client_id: &str, name: &str) -> Result<()> {
        let _guard = self.lock.lock().unwrap();
        let mut state = self.load_state()?;
        if let Some(c) = state.clients.iter_mut().find(|c| c.client_id == client_id) {
            c.client_name = Some(name.to_string());
            self.save_state(&state)?;
        }
        Ok(())
    }

    // ------------------------------------------------------------------------
    // Authorization codes
    // ------------------------------------------------------------------------

    pub fn issue_authorization_code(
        &self,
        client_id: &str,
        redirect_uri: &str,
        code_challenge: Option<String>,
        scopes: Vec<String>,
    ) -> Result<String> {
        let record = AuthorizationCodeRecord {
            code: generate_token(),
            client_id: client_id.to_string(),
            redirect_uri: redirect_uri.to_string(),
            code_challenge,
            scopes,
            created_at: Utc::now(),
        };

        let _guard = self.lock.lock().unwrap();
        let mut state = self.load_state()?;
        prune_expired_auth_codes(&mut state);
        state.authorization_codes.push(record.clone());
        self.save_state(&state)?;
        Ok(record.code)
    }

    pub fn exchange_authorization_code(
        &self,
        code: &str,
        code_verifier: Option<&str>,
    ) -> std::result::Result<TokenPair, ExchangeError> {
        let _guard = self.lock.lock().unwrap();
        let mut state = self
            .load_state()
            .map_err(|e| ExchangeError::Io(e.to_string()))?;
        prune_expired_auth_codes(&mut state);

        // Find and remove the code (single-use).
        let pos = state
            .authorization_codes
            .iter()
            .position(|c| c.code == code)
            .ok_or(ExchangeError::UnknownCode)?;
        let record = state.authorization_codes.remove(pos);

        // Expired (belt-and-suspenders — prune already dropped these).
        if Utc::now()
            .signed_duration_since(record.created_at)
            .num_seconds()
            > AUTH_CODE_LIFETIME_SECS
        {
            self.save_state(&state)
                .map_err(|e| ExchangeError::Io(e.to_string()))?;
            return Err(ExchangeError::Expired);
        }

        // PKCE check.
        if let Some(challenge) = &record.code_challenge {
            let verifier = code_verifier.ok_or(ExchangeError::MissingVerifier)?;
            if !verify_pkce(verifier, challenge) {
                self.save_state(&state)
                    .map_err(|e| ExchangeError::Io(e.to_string()))?;
                return Err(ExchangeError::PkceFailed);
            }
        }
        // If no challenge was set and no verifier supplied, that's fine.

        // Mint refresh + access.
        let now = Utc::now();
        let refresh = RefreshTokenRecord {
            token: generate_token(),
            client_id: record.client_id.clone(),
            scopes: record.scopes.clone(),
            issued_at: now,
            expires_at: now + self.refresh_ttl,
        };
        let access = AccessTokenRecord {
            token: generate_token(),
            client_id: record.client_id.clone(),
            refresh_token: refresh.token.clone(),
            scopes: record.scopes.clone(),
            issued_at: now,
            expires_at: now + self.access_ttl,
        };

        let pair = TokenPair {
            access_token: access.token.clone(),
            refresh_token: refresh.token.clone(),
            expires_in: self.access_ttl.num_seconds(),
            scopes: record.scopes.clone(),
        };

        state.refresh_tokens.push(refresh);
        state.access_tokens.push(access);

        self.save_state(&state)
            .map_err(|e| ExchangeError::Io(e.to_string()))?;

        Ok(pair)
    }

    // ------------------------------------------------------------------------
    // Access token validation
    // ------------------------------------------------------------------------

    pub fn validate_access_token(
        &self,
        token: &str,
    ) -> std::result::Result<ValidatedToken, ValidateError> {
        let _guard = self.lock.lock().unwrap();
        let state = self.load_state().map_err(|_| ValidateError::Unknown)?;

        let record = state
            .access_tokens
            .iter()
            .find(|t| t.token == token)
            .ok_or(ValidateError::Unknown)?;

        if Utc::now() >= record.expires_at {
            return Err(ValidateError::Expired);
        }

        let client_name = state
            .clients
            .iter()
            .find(|c| c.client_id == record.client_id)
            .and_then(|c| c.client_name.clone());

        Ok(ValidatedToken {
            client_id: record.client_id.clone(),
            client_name,
            scopes: record.scopes.clone(),
        })
    }

    // ------------------------------------------------------------------------
    // Refresh
    // ------------------------------------------------------------------------

    pub fn refresh_access_token(
        &self,
        refresh_token: &str,
    ) -> std::result::Result<TokenPair, RefreshError> {
        let _guard = self.lock.lock().unwrap();
        let mut state = self
            .load_state()
            .map_err(|e| RefreshError::Io(e.to_string()))?;

        let record = state
            .refresh_tokens
            .iter()
            .find(|t| t.token == refresh_token)
            .cloned()
            .ok_or(RefreshError::Unknown)?;

        if Utc::now() >= record.expires_at {
            return Err(RefreshError::Expired);
        }

        let now = Utc::now();
        let new_access = AccessTokenRecord {
            token: generate_token(),
            client_id: record.client_id.clone(),
            refresh_token: record.token.clone(),
            scopes: record.scopes.clone(),
            issued_at: now,
            expires_at: now + self.access_ttl,
        };

        let pair = TokenPair {
            access_token: new_access.token.clone(),
            refresh_token: record.token.clone(),
            expires_in: self.access_ttl.num_seconds(),
            scopes: record.scopes.clone(),
        };

        state.access_tokens.push(new_access);
        self.save_state(&state)
            .map_err(|e| RefreshError::Io(e.to_string()))?;

        Ok(pair)
    }

    // ------------------------------------------------------------------------
    // Revocation
    // ------------------------------------------------------------------------

    /// Revoke an access token. Returns true if a token was actually found & removed.
    pub fn revoke_access_token(&self, token: &str) -> Result<bool> {
        let _guard = self.lock.lock().unwrap();
        let mut state = self.load_state()?;
        let before = state.access_tokens.len();
        state.access_tokens.retain(|t| t.token != token);
        let removed = state.access_tokens.len() < before;
        if removed {
            self.save_state(&state)?;
        }
        Ok(removed)
    }

    /// Revoke a refresh token AND all access tokens minted from it.
    pub fn revoke_refresh_token(&self, token: &str) -> Result<bool> {
        let _guard = self.lock.lock().unwrap();
        let mut state = self.load_state()?;
        let before = state.refresh_tokens.len();
        state.refresh_tokens.retain(|t| t.token != token);
        let removed = state.refresh_tokens.len() < before;
        if removed {
            state.access_tokens.retain(|t| t.refresh_token != token);
            self.save_state(&state)?;
        }
        Ok(removed)
    }

    /// Revoke every active access token whose value starts with `prefix`.
    /// Returns the number of tokens removed. Owner CLI affordance:
    /// `tl hub tokens revoke <prefix>`.
    pub fn revoke_access_token_by_prefix(&self, prefix: &str) -> Result<usize> {
        let _guard = self.lock.lock().unwrap();
        let mut state = self.load_state()?;
        let before = state.access_tokens.len();
        state.access_tokens.retain(|t| !t.token.starts_with(prefix));
        let removed = before - state.access_tokens.len();
        if removed > 0 {
            self.save_state(&state)?;
        }
        Ok(removed)
    }

    /// Revoke every access + refresh token for a client.
    pub fn revoke_client(&self, client_id: &str) -> Result<()> {
        let _guard = self.lock.lock().unwrap();
        let mut state = self.load_state()?;
        state.access_tokens.retain(|t| t.client_id != client_id);
        state.refresh_tokens.retain(|t| t.client_id != client_id);
        self.save_state(&state)?;
        Ok(())
    }

    // ------------------------------------------------------------------------
    // Device-code flow (RFC 8628)
    //
    // Used by `tl hub link` so the CLI doesn't need to spin up a localhost
    // listener for the OAuth redirect. Flow:
    //   1. CLI calls `start_device_authorization` (via POST /device/code).
    //   2. CLI prints the verification URL (with embedded user_code) and
    //      starts polling `poll_device_token` (via POST /token grant
    //      device_code) every `interval` seconds.
    //   3. User opens the URL, hits the same /authorize page MCP clients use,
    //      master-pastes (self-hosted) or signs in (Pro), and that handler
    //      calls `authorize_device_session` to mint tokens against the
    //      pending session.
    //   4. The next CLI poll sees status=Authorized and gets the tokens.
    //      The session is consumed at that point.
    // ------------------------------------------------------------------------

    /// Step 1 — CLI initiates a device-code session. Returns the codes the
    /// CLI prints to the user plus the polling parameters.
    pub fn start_device_authorization(
        &self,
        client_id: &str,
        requested_scopes: Vec<String>,
    ) -> Result<DeviceAuthorization> {
        let now = Utc::now();
        let expires_at = now + Duration::seconds(DEVICE_CODE_LIFETIME_SECS);
        let record = DeviceSessionRecord {
            device_code: generate_token(),
            user_code: generate_user_code(),
            client_id: client_id.to_string(),
            requested_scopes,
            created_at: now,
            expires_at,
            status: DeviceSessionStatus::Pending,
        };

        let _guard = self.lock.lock().unwrap();
        let mut state = self.load_state()?;
        prune_expired_device_sessions(&mut state);
        state.device_sessions.push(record.clone());
        self.save_state(&state)?;

        Ok(DeviceAuthorization {
            device_code: record.device_code,
            user_code: record.user_code,
            expires_in: DEVICE_CODE_LIFETIME_SECS,
            interval: DEVICE_CODE_POLL_INTERVAL_SECS,
        })
    }

    /// Look up a pending device-code session by user_code. Used by the
    /// /authorize HTML page to render which client and scopes are pending.
    /// Returns `None` if no pending session matches (expired, unknown, or
    /// already consumed).
    pub fn find_pending_device_session(
        &self,
        user_code: &str,
    ) -> Result<Option<DeviceSessionInfo>> {
        let _guard = self.lock.lock().unwrap();
        let mut state = self.load_state()?;
        prune_expired_device_sessions(&mut state);

        let normalized = normalize_user_code(user_code);
        let session = state.device_sessions.iter().find(|s| {
            s.user_code == normalized && matches!(s.status, DeviceSessionStatus::Pending)
        });

        let info = session.map(|s| {
            let client_name = state
                .clients
                .iter()
                .find(|c| c.client_id == s.client_id)
                .and_then(|c| c.client_name.clone());
            DeviceSessionInfo {
                user_code: s.user_code.clone(),
                client_id: s.client_id.clone(),
                client_name,
                scopes: s.requested_scopes.clone(),
            }
        });

        // Save in case prune mutated state.
        self.save_state(&state)?;
        Ok(info)
    }

    /// Step 3 — browser-side authorization completed. Mint tokens for the
    /// session matching `user_code`. Returns Err if no pending session.
    pub fn authorize_device_session(&self, user_code: &str, scopes: Vec<String>) -> Result<()> {
        let _guard = self.lock.lock().unwrap();
        let mut state = self.load_state()?;
        prune_expired_device_sessions(&mut state);

        let normalized = normalize_user_code(user_code);
        let pos = state
            .device_sessions
            .iter()
            .position(|s| {
                s.user_code == normalized && matches!(s.status, DeviceSessionStatus::Pending)
            })
            .ok_or_else(|| anyhow::anyhow!("Unknown or expired device session"))?;

        let session = state.device_sessions[pos].clone();

        // Mint tokens with the scopes granted by the authorize step. Caller
        // passes the scopes (could be a subset of session.requested_scopes
        // if we ever build a "downgrade" UI; today they match).
        let now = Utc::now();
        let refresh = RefreshTokenRecord {
            token: generate_token(),
            client_id: session.client_id.clone(),
            scopes: scopes.clone(),
            issued_at: now,
            expires_at: now + self.refresh_ttl,
        };
        let access = AccessTokenRecord {
            token: generate_token(),
            client_id: session.client_id.clone(),
            refresh_token: refresh.token.clone(),
            scopes: scopes.clone(),
            issued_at: now,
            expires_at: now + self.access_ttl,
        };

        // Update the session in place to Authorized. The CLI's next poll
        // will pick up the tokens and remove the session.
        state.device_sessions[pos].status = DeviceSessionStatus::Authorized {
            access_token: access.token.clone(),
            refresh_token: refresh.token.clone(),
            scopes: scopes.clone(),
        };

        state.refresh_tokens.push(refresh);
        state.access_tokens.push(access);
        self.save_state(&state)?;
        Ok(())
    }

    /// Step 4 — CLI polls. Returns Pending until the user authorizes; on
    /// success returns the token pair and consumes the session.
    pub fn poll_device_token(
        &self,
        device_code: &str,
    ) -> std::result::Result<TokenPair, DeviceCodeError> {
        let _guard = self.lock.lock().unwrap();
        let mut state = self
            .load_state()
            .map_err(|e| DeviceCodeError::Io(e.to_string()))?;
        prune_expired_device_sessions(&mut state);

        let pos = state
            .device_sessions
            .iter()
            .position(|s| s.device_code == device_code)
            .ok_or(DeviceCodeError::Unknown)?;

        let session = state.device_sessions[pos].clone();

        if Utc::now() >= session.expires_at {
            // Session expired but wasn't pruned yet.
            state.device_sessions.remove(pos);
            self.save_state(&state)
                .map_err(|e| DeviceCodeError::Io(e.to_string()))?;
            return Err(DeviceCodeError::Expired);
        }

        match session.status {
            DeviceSessionStatus::Pending => Err(DeviceCodeError::AuthorizationPending),
            DeviceSessionStatus::Denied => {
                state.device_sessions.remove(pos);
                self.save_state(&state)
                    .map_err(|e| DeviceCodeError::Io(e.to_string()))?;
                Err(DeviceCodeError::AccessDenied)
            }
            DeviceSessionStatus::Authorized {
                access_token,
                refresh_token,
                scopes,
            } => {
                // Consume — single use.
                state.device_sessions.remove(pos);
                self.save_state(&state)
                    .map_err(|e| DeviceCodeError::Io(e.to_string()))?;
                Ok(TokenPair {
                    access_token,
                    refresh_token,
                    expires_in: self.access_ttl.num_seconds(),
                    scopes,
                })
            }
        }
    }

    // ------------------------------------------------------------------------
    // Listing
    // ------------------------------------------------------------------------

    pub fn list_tokens(&self) -> Result<Vec<TokenSummary>> {
        let _guard = self.lock.lock().unwrap();
        let state = self.load_state()?;
        let now = Utc::now();
        Ok(state
            .access_tokens
            .iter()
            .filter(|t| t.expires_at > now)
            .map(|t| {
                let client_name = state
                    .clients
                    .iter()
                    .find(|c| c.client_id == t.client_id)
                    .and_then(|c| c.client_name.clone());
                TokenSummary {
                    access_token_prefix: t.token.chars().take(8).collect(),
                    client_id: t.client_id.clone(),
                    client_name,
                    scopes: t.scopes.clone(),
                    issued_at: t.issued_at,
                    expires_at: t.expires_at,
                }
            })
            .collect())
    }

    /// One row per registered client that currently has at least one
    /// non-expired access token. Surfaces what Pro's "Devices / Apps"
    /// dashboard needs: who connected, what they can do, when they last hit
    /// the hub. The `kind` field is derived from scopes (pull/push ⟹
    /// device, read/write ⟹ app) so the same JSON powers both UI lists.
    pub fn list_active_clients(&self) -> Result<Vec<ClientSummary>> {
        let _guard = self.lock.lock().unwrap();
        let state = self.load_state()?;
        let now = Utc::now();

        let mut summaries: Vec<ClientSummary> = Vec::new();

        for client in &state.clients {
            // Pick the freshest non-expired access token for this client (if any).
            let active_token = state
                .access_tokens
                .iter()
                .filter(|t| t.client_id == client.client_id && t.expires_at > now)
                .max_by_key(|t| t.issued_at);

            let Some(tok) = active_token else { continue };

            let kind = ClientKind::from_scopes(&tok.scopes);

            summaries.push(ClientSummary {
                client_id: client.client_id.clone(),
                name: client.client_name.clone(),
                kind,
                scopes: tok.scopes.clone(),
                created_at: client.registered_at,
                last_token_issued_at: tok.issued_at,
                expires_at: tok.expires_at,
            });
        }

        // Newest first.
        summaries.sort_by(|a, b| b.last_token_issued_at.cmp(&a.last_token_issued_at));
        Ok(summaries)
    }

    // ------------------------------------------------------------------------
    // File I/O
    // ------------------------------------------------------------------------

    fn state_path(&self) -> PathBuf {
        self.treeline_dir.join(STATE_FILE)
    }

    fn load_state(&self) -> Result<OAuthState> {
        let path = self.state_path();
        if !path.exists() {
            return Ok(OAuthState::default());
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        serde_json::from_str(&content).context("Failed to parse oauth.json")
    }

    fn save_state(&self, state: &OAuthState) -> Result<()> {
        fs::create_dir_all(&self.treeline_dir)?;
        let path = self.state_path();
        let tmp = path.with_extension("json.tmp");

        let content = serde_json::to_string_pretty(state)?;
        {
            let mut f = fs::File::create(&tmp)
                .with_context(|| format!("Failed to create {}", tmp.display()))?;
            f.write_all(content.as_bytes())?;
            f.sync_all()?;
        }
        fs::rename(&tmp, &path)
            .with_context(|| format!("Failed to rename {} -> {}", tmp.display(), path.display()))?;
        Ok(())
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn generate_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.gen::<u8>()).collect();
    hex::encode(bytes)
}

fn verify_pkce(verifier: &str, challenge: &str) -> bool {
    use base64::Engine;
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    let computed = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash);
    computed == challenge
}

fn prune_expired_auth_codes(state: &mut OAuthState) {
    let cutoff = Utc::now() - Duration::seconds(AUTH_CODE_LIFETIME_SECS);
    state.authorization_codes.retain(|c| c.created_at >= cutoff);
}

fn prune_expired_device_sessions(state: &mut OAuthState) {
    let now = Utc::now();
    state.device_sessions.retain(|s| s.expires_at > now);
}

/// User-typeable codes: 8 chars from a Crockford-ish alphabet (no I/L/O/0/1
/// to avoid confusion), formatted XXXX-XXXX. Compared case-insensitively
/// and dash-insensitively at lookup time.
fn generate_user_code() -> String {
    use rand::Rng;
    const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let mut rng = rand::thread_rng();
    let chars: String = (0..8)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect();
    format!("{}-{}", &chars[..4], &chars[4..])
}

/// Strip dashes, uppercase, then re-insert dash. So `gztm-xkqr`,
/// `GZTMXKQR`, and `GZTM-XKQR` all match.
fn normalize_user_code(input: &str) -> String {
    let cleaned: String = input
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_uppercase())
        .collect();
    if cleaned.len() == 8 {
        format!("{}-{}", &cleaned[..4], &cleaned[4..])
    } else {
        cleaned
    }
}

fn access_ttl_from_env() -> Duration {
    let days = std::env::var("TL_HUB_ACCESS_TOKEN_TTL_DAYS")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(DEFAULT_ACCESS_TTL_DAYS);
    Duration::days(days)
}

fn refresh_ttl_from_env() -> Duration {
    let days = std::env::var("TL_HUB_REFRESH_TOKEN_TTL_DAYS")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(DEFAULT_REFRESH_TTL_DAYS);
    Duration::days(days)
}
