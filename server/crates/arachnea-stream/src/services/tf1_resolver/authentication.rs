//! TF1 login, bootstrap and session-cache operations.

use super::*;

/// Returns a cached TF1 session or performs Gigya and TF1 token login.
///
/// Cache misses are serialized by `TF1_LOGIN_LOCK` so concurrent calls do not
/// submit duplicate credential requests.
///
/// # Arguments
/// * `http_client` - HTTP client used for bootstrap, login and token calls.
/// * `credentials_store` - Store providing the TF1 login and password.
///
/// # Errors
/// Returns an error when credentials are missing, Gigya rejects the login,
/// a required user field is absent, or token exchange fails.
///
/// # Returns
/// A session containing the TF1 bearer token.
pub(super) async fn get_or_login_session(
    http_client: &HttpClient,
    credentials_store: &dyn CredentialsStore,
) -> Result<Tf1Session> {
    let (login, password) = load_credentials(credentials_store)?;

    if let Some(session) = load_cached_session(&login) {
        return Ok(session);
    }

    let _login_guard = TF1_LOGIN_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    if let Some(session) = load_cached_session(&login) {
        return Ok(session);
    }

    bootstrap_gigya_session(http_client).await?;

    let login_body = serde_urlencoded::to_string([
        ("loginID", login.as_str()),
        ("password", password.as_str()),
        ("sessionExpiration", "31536000"),
        ("targetEnv", "jssdk"),
        ("include", "identities-all,data,profile,preferences,"),
        ("includeUserInfo", "true"),
        ("loginMode", "standard"),
        ("lang", "fr"),
        ("APIKey", TF1_GIGYA_API_KEY),
        ("sdk", "js_latest"),
        ("authMode", "cookie"),
        ("pageURL", TF1_BASE_URL),
        ("sdkBuild", "13987"),
        ("format", "json"),
    ])
    .context("Failed to serialize the TF1 login payload.")?;

    let login_headers = HashMap::from([
        (
            "content-type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        ),
        ("referer".to_string(), TF1_BASE_URL.to_string()),
        (
            "user-agent".to_string(),
            crate::PLAYER_DEFAULT_USER_AGENT.to_string(),
        ),
    ]);

    let login_payload = http_client
        .get_json_for_request(
            http::Method::POST,
            TF1_LOGIN_URL,
            &login_headers,
            Some(&login_body),
        )
        .await
        .context("Failed to submit TF1 credentials to Gigya.")?;
    bail_on_gigya_error(&login_payload, "TF1 rejected the configured credentials.")?;

    let uid = read_json_string(
        &login_payload,
        &["userInfo", "UID"],
        "Missing TF1 UID in Gigya login response.",
    )?;
    let signature = read_json_string(
        &login_payload,
        &["userInfo", "UIDSignature"],
        "Missing TF1 UID signature in Gigya login response.",
    )?;
    let timestamp = read_json_string(
        &login_payload,
        &["userInfo", "signatureTimestamp"],
        "Missing TF1 signature timestamp in Gigya login response.",
    )?;

    let token_body = json!({
        "uid": uid,
        "signature": signature,
        "timestamp": timestamp.parse::<u64>().unwrap_or_default(),
        "consent_ids": [
            "1", "2", "3", "4", "10001", "10003", "10005", "10007", "10013",
            "10015", "10017", "10019", "10009", "10011", "13002", "13001",
            "10004", "10014", "10016", "10018", "10020", "10010", "10012",
            "10006", "10008"
        ]
    })
    .to_string();

    let token_headers = HashMap::from([
        ("content-type".to_string(), "application/json".to_string()),
        (
            "user-agent".to_string(),
            crate::PLAYER_DEFAULT_USER_AGENT.to_string(),
        ),
    ]);

    let token_payload = http_client
        .get_json_for_request(
            http::Method::POST,
            TF1_TOKEN_URL,
            &token_headers,
            Some(&token_body),
        )
        .await
        .context("Failed to exchange TF1 Gigya identifiers for a TF1 web token.")?;
    let token = read_json_string(
        &token_payload,
        &["token"],
        "Missing TF1 bearer token in TF1 token response.",
    )?;

    let session = Tf1Session { token };
    save_cached_session(&login, &session);
    Ok(session)
}

/// Initializes the Gigya web-sdk session before credential submission.
///
/// # Arguments
/// * `http_client` - HTTP client whose cookie jar receives the bootstrap state.
///
/// # Errors
/// Returns an error when the bootstrap query cannot be serialized or the
/// request fails.
pub(super) async fn bootstrap_gigya_session(http_client: &HttpClient) -> Result<()> {
    let query = serde_urlencoded::to_string([
        ("apiKey", TF1_GIGYA_API_KEY),
        ("pageURL", "https://www.tf1.fr/"),
        ("sd", "js_latest"),
        ("sdkBuild", "13987"),
        ("format", "json"),
    ])
    .context("Failed to serialize the TF1 Gigya bootstrap query string.")?;
    let bootstrap_url = format!("{}?{}", TF1_BOOTSTRAP_URL, query);

    let bootstrap_headers = HashMap::from([
        ("referer".to_string(), TF1_BASE_URL.to_string()),
        (
            "user-agent".to_string(),
            crate::PLAYER_DEFAULT_USER_AGENT.to_string(),
        ),
    ]);

    let started = Instant::now();
    http_client
        .query_http_for_request(http::Method::GET, &bootstrap_url, &bootstrap_headers, None)
        .await
        .map_err(|error| {
            tracing::warn!(target: "arachnea_stream::services::tf1_resolver",
                endpoint = TF1_BOOTSTRAP_URL,
                elapsed_ms = started.elapsed().as_millis(),
                error = %format!("{error:#}"),
                "TF1 Gigya bootstrap request failed"
            );
            error
        })
        .context("Failed to bootstrap the TF1 Gigya web session.")?;

    Ok(())
}

/// Fails when a Gigya response reports a non-zero error code.
///
/// # Arguments
/// * `payload` - Decoded Gigya response.
/// * `default_message` - Fallback message when no server detail is available.
///
/// # Errors
/// Returns the server-provided error detail or `default_message` when
/// `errorCode` is not zero.
pub(super) fn bail_on_gigya_error(payload: &Value, default_message: &str) -> Result<()> {
    let error_code = payload
        .get("errorCode")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if error_code == 0 {
        return Ok(());
    }

    let message = payload
        .get("errorMessage")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            payload
                .get("errorDetails")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or(default_message);

    bail!(message.to_string())
}

/// Loads the configured TF1 login and password.
///
/// # Arguments
/// * `credentials_store` - Store queried for the TF1 service credentials.
///
/// # Errors
/// Returns an error when the store cannot be read or has no TF1 credentials.
///
/// # Returns
/// The credential login and password.
pub(super) fn load_credentials(
    credentials_store: &dyn CredentialsStore,
) -> Result<(String, String)> {
    let credentials = credentials_store
        .get_credentials(TF1_SERVICE_ID)
        .with_context(|| {
            format!(
                "Failed to load credentials for service `{}` from the configured store.",
                TF1_SERVICE_ID
            )
        })?
        .with_context(|| {
            format!(
                "Missing credentials for service `{}` in the configured store.",
                TF1_SERVICE_ID
            )
        })?;

    Ok((credentials.login, credentials.password))
}

/// Returns a non-expired session for `login`, pruning expired entries.
///
/// # Arguments
/// * `login` - TF1 account login used as the cache key.
///
/// # Returns
/// The cached session, or `None` when it is absent, expired, or the cache
/// mutex is poisoned.
pub(super) fn load_cached_session(login: &str) -> Option<Tf1Session> {
    let cache = TF1_SESSION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache_guard) = cache.lock() else {
        return None;
    };

    let now = Instant::now();
    cache_guard.retain(|_, entry| entry.expires_at > now);
    cache_guard.get(login).map(|entry| entry.session.clone())
}

/// Caches a session for `login` using `TF1_SESSION_TTL`.
///
/// # Arguments
/// * `login` - TF1 account login used as the cache key.
/// * `session` - Session to store.
///
/// The write is skipped when the cache mutex is poisoned.
pub(super) fn save_cached_session(login: &str, session: &Tf1Session) {
    let cache = TF1_SESSION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache_guard) = cache.lock() else {
        return;
    };

    cache_guard.insert(
        login.to_string(),
        CachedTf1Session {
            session: session.clone(),
            expires_at: Instant::now() + TF1_SESSION_TTL,
        },
    );
}
