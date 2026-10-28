//! RMC/BFM Play Cognito authentication, session cache and authenticated BFF requests.

use anyhow::{bail, Context, Result};
use arachnea_core::persistence::CredentialsStore;
use arachnea_scrapyfy::HttpClient;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::Instant,
};

use super::payload::{describe_value, preview_error_body, read_string};
use super::{
    playback_headers, CONNECT_CLIENT_ID, GATEWAY_URL, SERVICE_ID, SESSION_TTL, USER_AGENT,
};

/// Process-wide cache of authenticated RMC/BFM Play sessions.
static SESSION_CACHE: OnceLock<Mutex<HashMap<String, CachedSession>>> = OnceLock::new();
/// Serializes cache misses during the RMC/BFM Play login flow.
static LOGIN_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

/// Cognito tokens returned by the Connect gateway.
#[derive(Clone)]
pub(super) struct CognitoSession {
    id_token: String,
    access_token: String,
    #[allow(dead_code)]
    refresh_token: String,
}

/// Cached Cognito session with its expiry instant.
struct CachedSession {
    session: CognitoSession,
    expires_at: Instant,
}

/// Returns a cached session or signs in through the Connect gateway.
///
/// # Arguments
/// * `http_client` - HTTP client used for the sign-in request.
/// * `credentials_store` - Store providing the RMC/BFM Play credentials.
///
/// # Errors
/// Returns an error when credentials are missing or blank, or sign-in fails.
///
/// # Returns
/// The authenticated Cognito session.
pub(super) async fn get_or_login_session(
    http_client: &HttpClient,
    credentials_store: &dyn CredentialsStore,
) -> Result<CognitoSession> {
    let credentials = credentials_store
        .get_credentials(SERVICE_ID)
        .context("Failed to load RMC BFM Play credentials.")?
        .context("Missing credentials for service `rmcbfmplay-fr`.")?;
    let login = credentials.login.trim();
    let password = credentials.password.trim();
    if login.is_empty() || password.is_empty() {
        bail!("RMC BFM Play credentials must include a login and password.");
    }
    if let Some(session) = load_cached_session(login) {
        return Ok(session);
    }
    let _guard = LOGIN_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    if let Some(session) = load_cached_session(login) {
        return Ok(session);
    }

    let session = sign_in(http_client, login, password).await?;
    save_cached_session(login, &session);
    Ok(session)
}

/// Signs in through the Connect gateway, mirroring the Connect login page
/// (`sign_in` endpoint, email lowercased/trimmed, password trimmed).
///
/// # Arguments
/// * `http_client` - HTTP client used for the gateway request.
/// * `login` - Account email.
/// * `password` - Account password.
///
/// # Errors
/// Returns an error when the gateway rejects the credentials or omits required
/// Cognito tokens.
///
/// # Returns
/// The authenticated Cognito session.
async fn sign_in(http_client: &HttpClient, login: &str, password: &str) -> Result<CognitoSession> {
    let body = json!({
        "endpoint": "sign_in",
        "payload": {
            "email": login.trim().to_lowercase(),
            "password": password.trim(),
            "clientId": CONNECT_CLIENT_ID,
        },
        "withRawResponse": true,
    })
    .to_string();
    let response = http_client
        .send_for_request(
            http::Method::POST,
            GATEWAY_URL,
            &gateway_headers(),
            Some(&body),
        )
        .await
        .context("Failed to authenticate with RMC BFM Play.")?;
    let status = response.status();
    let text = response
        .text()
        .await
        .context("Failed to authenticate with RMC BFM Play.")?;
    if !status.is_success() {
        let (kind, message) = parse_gateway_error(&text);
        bail!("RMC BFM Play rejected the configured credentials ({kind}): {message}");
    }
    let payload: Value = serde_json::from_str(&text).with_context(|| {
        format!(
            "Failed to authenticate with RMC BFM Play (HTTP {status}): invalid JSON: {}",
            preview_error_body(&text)
        )
    })?;
    let result = payload.pointer("/AuthenticationResult").with_context(|| {
        format!(
            "RMC BFM Play sign-in did not return authentication tokens ({})",
            describe_value(&payload)
        )
    })?;
    Ok(CognitoSession {
        id_token: read_string(result, "/IdToken", "Missing RMC BFM Play ID token.")?,
        access_token: read_string(result, "/AccessToken", "Missing RMC BFM Play access token.")?,
        refresh_token: read_string(
            result,
            "/RefreshToken",
            "Missing RMC BFM Play refresh token.",
        )?,
    })
}

/// Fetches a BFF document, trying the known Cognito token transports in order.
///
/// The exact transport the `/api/bff` server proxy injects is not documented;
/// each attempt is reported, and the successful variant is logged (never the
/// token), so the chain can be locked to the working variant once observed.
///
/// # Arguments
/// * `http_client` - HTTP client used for the request.
/// * `session` - Authenticated Cognito session providing candidate tokens.
/// * `url` - BFF document URL.
/// * `require_video` - Whether a successful response must contain `video`.
///
/// # Errors
/// Returns an error when every transport fails or lacks the required video.
///
/// # Returns
/// The first successful decoded BFF payload.
pub(super) async fn fetch_bff_json(
    http_client: &HttpClient,
    session: &CognitoSession,
    url: &str,
    require_video: bool,
) -> Result<Value> {
    let variants: [(&str, String); 4] = [
        ("bearer-access", format!("Bearer {}", session.access_token)),
        ("bearer-id", format!("Bearer {}", session.id_token)),
        ("raw-access", session.access_token.clone()),
        ("raw-id", session.id_token.clone()),
    ];
    let mut attempts = Vec::new();
    for (name, authorization) in &variants {
        let mut headers = playback_headers();
        headers.insert("authorization".to_string(), authorization.clone());
        let response = match http_client
            .send_for_request(http::Method::GET, url, &headers, None)
            .await
        {
            Ok(response) => response,
            Err(error) => {
                attempts.push(format!("{name}: transport {error:#}"));
                continue;
            }
        };
        let status = response.status();
        let text = match response.text().await {
            Ok(text) => text,
            Err(error) => {
                attempts.push(format!("{name}: unreadable body {error:#}"));
                continue;
            }
        };
        if !status.is_success() {
            attempts.push(format!(
                "{name}: HTTP {status}: {}",
                preview_error_body(&text)
            ));
            continue;
        }
        let payload: Value = match serde_json::from_str(&text) {
            Ok(payload) => payload,
            Err(_) => {
                attempts.push(format!(
                    "{name}: HTTP {status} invalid JSON: {}",
                    preview_error_body(&text)
                ));
                continue;
            }
        };
        if require_video
            && payload
                .pointer("/video")
                .filter(|video| video.is_object())
                .is_none()
        {
            attempts.push(format!("{name}: HTTP {status} without video payload"));
            continue;
        }
        tracing::info!(target: "arachnea_stream::services::rmcbfmplay_resolver",
            variant = name,
            url = url,
            "RMC BFM Play authenticated BFF request succeeded"
        );
        return Ok(payload);
    }
    bail!(
        "RMC BFM Play BFF request failed for `{url}`: {}",
        attempts.join("; ")
    )
}

/// Builds the headers used for Connect gateway requests.
///
/// # Returns
/// JSON request and acceptance headers with the resolver user-agent.
fn gateway_headers() -> HashMap<String, String> {
    HashMap::from([
        ("user-agent".to_string(), USER_AGENT.to_string()),
        ("content-type".to_string(), "application/json".to_string()),
        (
            "accept".to_string(),
            "application/json, text/plain, */*".to_string(),
        ),
    ])
}

/// Reads the Cognito error kind and message without exposing credentials.
///
/// # Arguments
/// * `body` - Gateway error response body.
///
/// # Returns
/// The normalized error kind and a safe message.
fn parse_gateway_error(body: &str) -> (String, String) {
    if let Ok(payload) = serde_json::from_str::<Value>(body) {
        let kind = payload
            .get("__type")
            .or_else(|| payload.get("errorCode"))
            .or_else(|| payload.get("error"))
            .and_then(Value::as_str)
            .map(|kind| kind.rsplit('#').next().unwrap_or(kind).trim().to_string())
            .filter(|kind| !kind.is_empty())
            .unwrap_or_else(|| "UnknownError".to_string());
        let message = payload
            .get("message")
            .or_else(|| payload.get("errorMessage"))
            .or_else(|| payload.get("error"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|message| !message.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| preview_error_body(body));
        return (kind, message);
    }
    ("UnknownError".to_string(), preview_error_body(body))
}

/// Caches a session for `login` using `SESSION_TTL`.
///
/// # Arguments
/// * `login` - Account email used as the cache key.
/// * `session` - Session to store.
///
/// The write is skipped when the cache mutex is poisoned.
fn save_cached_session(login: &str, session: &CognitoSession) {
    let cache = SESSION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut cache) = cache.lock() {
        cache.retain(|_, entry| entry.expires_at > Instant::now());
        cache.insert(
            login.to_string(),
            CachedSession {
                session: session.clone(),
                expires_at: Instant::now() + SESSION_TTL,
            },
        );
    }
}

/// Loads a non-expired session, pruning expired entries.
///
/// # Arguments
/// * `login` - Account email used as the cache key.
///
/// # Returns
/// The cached session, or `None` when absent, expired, or the cache mutex is
/// poisoned.
fn load_cached_session(login: &str) -> Option<CognitoSession> {
    let cache = SESSION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache) = cache.lock() else {
        return None;
    };
    cache.retain(|_, entry| entry.expires_at > Instant::now());
    cache.get(login).map(|entry| entry.session.clone())
}
