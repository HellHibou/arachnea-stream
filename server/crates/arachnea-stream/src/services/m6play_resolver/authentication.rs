//! M6 login, API-key discovery, playback tokens and process-local caches.

use anyhow::{anyhow, bail, Context, Result};
use arachnea_core::persistence::CredentialsStore;
use arachnea_scrapyfy::HttpClient;
use regex::Regex;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::Instant,
};
use tokio::sync::Mutex as AsyncMutex;

use super::{
    read_json_string, M6PLAY_SERVICE_ID, SIXPLAY_API_KEY_FALLBACK, SIXPLAY_API_KEY_FALLBACK_TTL,
    SIXPLAY_API_KEY_LOOKUP_TIMEOUT, SIXPLAY_API_KEY_TTL, SIXPLAY_BUNDLE_URL_TEMPLATE,
    SIXPLAY_CALLBACK_NAME, SIXPLAY_CLIENT_RELEASE, SIXPLAY_CUSTOMER_NAME, SIXPLAY_DEVICE_ID,
    SIXPLAY_LOGIN_PAGE_URL, SIXPLAY_LOGIN_URL, SIXPLAY_SESSION_TTL,
    SIXPLAY_TOKEN_REPLAY_URL_TEMPLATE, SIXPLAY_TOKEN_UUID_URL,
};
use crate::PLAYER_ERROR_BODY_PREVIEW_LENGTH;

/// Shared regex used to find the M6 JavaScript bundle identifier.
static SIXPLAY_JS_ID_REGEX: OnceLock<Regex> = OnceLock::new();
/// Shared regex used to find the public M6 API key in the bundle.
static SIXPLAY_API_KEY_REGEX: OnceLock<Regex> = OnceLock::new();
/// Process-local cache for the public M6 API key.
static SIXPLAY_API_KEY_CACHE: OnceLock<Mutex<Option<CachedSixPlayApiKey>>> = OnceLock::new();
/// Process-wide cache of authenticated M6 sessions keyed by login.
static SIXPLAY_SESSION_CACHE: OnceLock<Mutex<HashMap<String, CachedSixPlaySession>>> =
    OnceLock::new();
/// Serializes cache misses during the M6 login flow.
static SIXPLAY_LOGIN_LOCK: OnceLock<AsyncMutex<()>> = OnceLock::new();

/// Authenticated M6 account and login tokens.
#[derive(Clone)]
pub(super) struct SixPlaySession {
    /// Account identifier returned by Gigya.
    pub(super) account_id: String,
    /// Login token used to acquire live and replay playback tokens.
    pub(super) login_token: String,
}

/// Cached M6 session with its expiry instant.
struct CachedSixPlaySession {
    session: SixPlaySession,
    expires_at: Instant,
}

/// Process-local public API key; contains no account credentials or session token.
struct CachedSixPlayApiKey {
    /// Discovered key or existing bundled fallback.
    key: String,
    /// Monotonic expiry controlling the next optional web lookup.
    expires_at: Instant,
}

/// Returns a cached M6 session or performs the full Gigya login flow.
///
/// Cache misses are serialized by `SIXPLAY_LOGIN_LOCK`.
///
/// # Arguments
/// * `http_client` - HTTP client used for key discovery and login requests.
/// * `credentials_store` - Store providing the M6 login and password.
///
/// # Errors
/// Returns an error when credentials are missing, key discovery falls back and
/// login still fails, or required login fields are absent.
///
/// # Returns
/// A session containing the account and login token.
pub(super) async fn get_or_login_session(
    http_client: &HttpClient,
    credentials_store: &dyn CredentialsStore,
) -> Result<SixPlaySession> {
    let (login, password) = load_credentials(credentials_store)?;

    if let Some(session) = load_cached_session(&login) {
        return Ok(session);
    }

    let _login_guard = SIXPLAY_LOGIN_LOCK
        .get_or_init(|| AsyncMutex::new(()))
        .lock()
        .await;

    if let Some(session) = load_cached_session(&login) {
        return Ok(session);
    }

    let api_key = resolve_api_key(http_client).await;

    let payload = serde_urlencoded::to_string([
        ("loginID", login.as_str()),
        ("password", password.as_str()),
        ("apiKey", api_key.as_str()),
        ("format", "jsonp"),
        ("callback", SIXPLAY_CALLBACK_NAME),
    ])
    .context("Failed to serialize the 6play login payload.")?;

    let mut login_headers = HashMap::new();
    login_headers.insert(
        "content-type".to_string(),
        "application/x-www-form-urlencoded".to_string(),
    );
    login_headers.insert("referer".to_string(), SIXPLAY_LOGIN_PAGE_URL.to_string());

    let login_response = http_client
        .query_http_for_request(
            http::Method::POST,
            SIXPLAY_LOGIN_URL,
            &login_headers,
            Some(&payload),
        )
        .await
        .context("Failed to submit 6play credentials to Gigya.")?;

    let login_payload = parse_jsonp_payload(&login_response)?;
    if login_payload
        .get("errorCode")
        .and_then(Value::as_i64)
        .is_some_and(|code| code != 0)
    {
        invalidate_api_key(&api_key);
    }
    let account_id = read_json_string(
        &login_payload,
        &["UID"],
        "Missing 6play account identifier in login response.",
    )?;
    let account_signature = read_json_string(
        &login_payload,
        &["UIDSignature"],
        "Missing 6play account signature in login response.",
    )?;
    let account_timestamp = read_json_string(
        &login_payload,
        &["signatureTimestamp"],
        "Missing 6play signature timestamp in login response.",
    )?;

    let mut uuid_headers = HashMap::new();
    uuid_headers.insert(
        "x-auth-gigya-signature".to_string(),
        account_signature.clone(),
    );
    uuid_headers.insert(
        "x-auth-gigya-signature-timestamp".to_string(),
        account_timestamp,
    );
    uuid_headers.insert("x-auth-gigya-uid".to_string(), account_id.clone());
    uuid_headers.insert(
        "x-auth-device-id".to_string(),
        SIXPLAY_DEVICE_ID.to_string(),
    );
    uuid_headers.insert(
        "x-customer-name".to_string(),
        SIXPLAY_CUSTOMER_NAME.to_string(),
    );

    let uuid_payload =
        fetch_sixplay_front_auth_json(http_client, SIXPLAY_TOKEN_UUID_URL, &uuid_headers).await?;
    let login_token = read_json_string(
        &uuid_payload,
        &["token"],
        "Missing 6play login token in front-auth response.",
    )?;

    let session = SixPlaySession {
        account_id,
        login_token,
    };

    save_cached_session(&login, &session);
    Ok(session)
}

/// Fetches and decodes a 6play front-auth JSON response.
///
/// # Arguments
/// * `http_client` - HTTP client used for the request.
/// * `url` - Front-auth endpoint URL.
/// * `headers` - Authentication headers sent with the request.
///
/// # Errors
/// Returns an error when the request fails, the status is unsuccessful, or the
/// body is not valid JSON.
///
/// # Returns
/// The decoded front-auth payload.
async fn fetch_sixplay_front_auth_json(
    http_client: &HttpClient,
    url: &str,
    headers: &HashMap<String, String>,
) -> Result<Value> {
    let response = http_client
        .send_for_request(http::Method::GET, url, headers, None)
        .await
        .map_err(|error| anyhow!("Failed to fetch 6play front-auth token: {}", error))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .context("Failed to read the 6play front-auth response.")?;

    if !status.is_success() {
        bail!(
            "6play front-auth request failed with HTTP {}: {}",
            status,
            preview_error_body(&body)
        );
    }

    serde_json::from_str(&body).with_context(|| {
        format!(
            "Invalid 6play front-auth JSON returned by {} (HTTP {}): {}",
            url,
            status,
            preview_error_body(&body)
        )
    })
}

/// Reuses a public key or bounds the complete optional lookup before falling back.
/// Called under the login lock, so concurrent logins cannot duplicate discovery.
///
/// # Arguments
/// - `http_client`: Client retaining the caller's configured transport and TLS policy.
async fn resolve_api_key(http_client: &HttpClient) -> String {
    let cache = SIXPLAY_API_KEY_CACHE.get_or_init(|| Mutex::new(None));
    if let Some(key) = cache.lock().ok().and_then(|entry| {
        entry
            .as_ref()
            .filter(|entry| entry.expires_at > Instant::now())
            .map(|entry| entry.key.clone())
    }) {
        tracing::debug!(target: "arachnea_stream::services::m6play_resolver", "reusing cached M6 API key");
        return key;
    }

    let started = Instant::now();
    let (key, ttl, source) = match tokio::time::timeout(
        SIXPLAY_API_KEY_LOOKUP_TIMEOUT,
        Box::pin(fetch_api_key(http_client)),
    )
    .await
    {
        Ok(Ok(key)) => (key, SIXPLAY_API_KEY_TTL, "web"),
        result => {
            tracing::warn!(target: "arachnea_stream::services::m6play_resolver",
                failure_kind = if result.is_err() { "timeout" } else { "lookup" },
                timeout_ms = SIXPLAY_API_KEY_LOOKUP_TIMEOUT.as_millis(),
                "M6 API key lookup did not complete successfully; using bundled fallback"
            );
            (
                SIXPLAY_API_KEY_FALLBACK.to_string(),
                SIXPLAY_API_KEY_FALLBACK_TTL,
                "bundled_fallback",
            )
        }
    };
    if let Ok(mut entry) = cache.lock() {
        *entry = Some(CachedSixPlayApiKey {
            key: key.clone(),
            expires_at: Instant::now() + ttl,
        });
    }
    tracing::info!(target: "arachnea_stream::services::m6play_resolver",
        source,
        duration_ms = started.elapsed().as_millis(),
        ttl_seconds = ttl.as_secs(),
        "resolved M6 API key"
    );
    key
}

/// Clears only the key used by a rejected login, without replaying the POST.
///
/// # Arguments
/// - `rejected_key`: Key submitted in the completed Gigya request.
fn invalidate_api_key(rejected_key: &str) {
    let Some(cache) = SIXPLAY_API_KEY_CACHE.get() else {
        return;
    };
    if let Ok(mut entry) = cache.lock() {
        if entry
            .as_ref()
            .is_some_and(|entry| entry.key == rejected_key)
        {
            *entry = None;
            tracing::debug!(target: "arachnea_stream::services::m6play_resolver", "invalidated cached M6 API key after Gigya login rejection");
        }
    }
}

/// Fetches the public M6 API key from the login page and JavaScript bundle.
///
/// # Arguments
/// * `http_client` - HTTP client used for the page and bundle requests.
///
/// # Errors
/// Returns an error when the bundle identifier, bundle request, or API key is
/// missing.
///
/// # Returns
/// The API key extracted from the web bundle.
async fn fetch_api_key(http_client: &HttpClient) -> Result<String> {
    let login_page = http_client
        .query_http(http::Method::GET, SIXPLAY_LOGIN_PAGE_URL)
        .await?;
    let js_id_regex = SIXPLAY_JS_ID_REGEX.get_or_init(|| {
        Regex::new(r"main-(.*?)\.bundle\.js").expect("Invalid 6play JS identifier regex.")
    });
    let Some(js_id) = js_id_regex
        .captures(&login_page)
        .and_then(|captures| captures.get(1))
        .map(|capture| capture.as_str().to_string())
    else {
        bail!("Missing 6play bundle identifier.");
    };

    let bundle_url = SIXPLAY_BUNDLE_URL_TEMPLATE.replace("{}", &js_id);
    let bundle = http_client
        .query_http(http::Method::GET, &bundle_url)
        .await?;
    let api_key_regex = SIXPLAY_API_KEY_REGEX.get_or_init(|| {
        Regex::new(r#""eu1\.gigya\.com",key:"(.*?)""#).expect("Invalid 6play API key regex.")
    });

    api_key_regex
        .captures(&bundle)
        .and_then(|captures| captures.get(1))
        .map(|capture| capture.as_str().to_string())
        .ok_or_else(|| anyhow!("Missing 6play API key in web bundle."))
}

/// Fetches the DRM upfront token for an M6 replay video.
///
/// # Arguments
/// * `http_client` - HTTP client used for the request.
/// * `session` - Authenticated M6 session.
/// * `video_id` - M6 video identifier.
///
/// # Errors
/// Returns an error when the request fails or the response has no token.
///
/// # Returns
/// The replay upfront token.
pub(super) async fn fetch_upfront_token(
    http_client: &HttpClient,
    session: &SixPlaySession,
    video_id: &str,
) -> Result<String> {
    let token_url = SIXPLAY_TOKEN_REPLAY_URL_TEMPLATE
        .replacen("{}", &session.account_id, 1)
        .replacen("{}", video_id, 1);

    let mut headers = HashMap::new();
    headers.insert(
        "x-customer-name".to_string(),
        SIXPLAY_CUSTOMER_NAME.to_string(),
    );
    headers.insert(
        "x-client-release".to_string(),
        SIXPLAY_CLIENT_RELEASE.to_string(),
    );
    headers.insert(
        "authorization".to_string(),
        format!("Bearer {}", session.login_token),
    );

    let token_payload = http_client
        .get_json_for_request(http::Method::GET, &token_url, &headers, None)
        .await
        .context("Failed to fetch the 6play replay upfront token.")?;

    read_json_string(
        &token_payload,
        &["token"],
        "Missing 6play upfront token for replay playback.",
    )
}

/// Parses the JSON object inside an M6 JSONP response.
///
/// # Arguments
/// * `payload` - Raw JSONP text.
///
/// # Errors
/// Returns an error when the parentheses or inner JSON are invalid.
///
/// # Returns
/// The decoded JSON payload.
fn parse_jsonp_payload(payload: &str) -> Result<Value> {
    let trimmed = payload.trim();
    let Some(open_paren_index) = trimmed.find('(') else {
        bail!("Invalid 6play JSONP login payload.");
    };
    let Some(close_paren_index) = trimmed.rfind(')') else {
        bail!("Invalid 6play JSONP login payload.");
    };

    let json_payload = trimmed[open_paren_index + 1..close_paren_index].trim();
    serde_json::from_str(json_payload).context("Invalid JSON inside the 6play JSONP response.")
}

/// Produces a bounded preview of an HTTP error body.
///
/// # Arguments
/// * `body` - Raw response body.
///
/// # Returns
/// A trimmed preview capped by `PLAYER_ERROR_BODY_PREVIEW_LENGTH`, with an
/// ellipsis when longer, or `<empty body>` when blank.
fn preview_error_body(body: &str) -> String {
    let value = body.trim();
    if value.is_empty() {
        return "<empty body>".to_string();
    }

    let mut preview: String = value
        .chars()
        .take(PLAYER_ERROR_BODY_PREVIEW_LENGTH)
        .collect();
    if value.chars().count() > 300 {
        preview.push_str("...");
    }
    preview
}

/// Loads the configured M6 login and password.
///
/// # Arguments
/// * `credentials_store` - Store queried for the M6 service credentials.
///
/// # Errors
/// Returns an error when the store cannot be read or has no M6 credentials.
///
/// # Returns
/// The credential login and password.
fn load_credentials(credentials_store: &dyn CredentialsStore) -> Result<(String, String)> {
    let credentials = credentials_store
        .get_credentials(M6PLAY_SERVICE_ID)
        .with_context(|| {
            format!(
                "Failed to load credentials for service `{}` from the configured store.",
                M6PLAY_SERVICE_ID
            )
        })?
        .with_context(|| {
            format!(
                "Missing credentials for service `{}` in the configured store.",
                M6PLAY_SERVICE_ID
            )
        })?;

    Ok((credentials.login, credentials.password))
}

/// Loads a non-expired M6 session, pruning expired entries.
///
/// # Arguments
/// * `login` - Account login used as the cache key.
///
/// # Returns
/// The cached session, or `None` when absent, expired, or the cache mutex is
/// poisoned.
fn load_cached_session(login: &str) -> Option<SixPlaySession> {
    let cache = SIXPLAY_SESSION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache_guard) = cache.lock() else {
        return None;
    };

    let now = Instant::now();
    cache_guard.retain(|_, entry| entry.expires_at > now);
    cache_guard.get(login).map(|entry| entry.session.clone())
}

/// Caches an M6 session for `login` using `SIXPLAY_SESSION_TTL`.
///
/// # Arguments
/// * `login` - Account login used as the cache key.
/// * `session` - Session to store.
///
/// The write is skipped when the cache mutex is poisoned.
fn save_cached_session(login: &str, session: &SixPlaySession) {
    let cache = SIXPLAY_SESSION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache_guard) = cache.lock() else {
        return;
    };

    cache_guard.insert(
        login.to_string(),
        CachedSixPlaySession {
            session: session.clone(),
            expires_at: Instant::now() + SIXPLAY_SESSION_TTL,
        },
    );
}
