use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use rand::{distr::Alphanumeric, Rng};
use rquest::header::{HeaderValue, CONTENT_TYPE};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use arachnea_core::persistence::CredentialsStore;
use arachnea_scrapyfy::{HttpClient, ScraperAgregator};

use crate::services::player_resolver::{
    PlayerResolverEndpoints, PlayerStreamRequest, PlayerStreamResolver, ProxiedStreamResponse,
    ResolvedPlayerStream,
};

/// Stable source identifier for RTBF Auvio routing and credentials.
const RTBF_AUVIO_SERVICE_ID: &str = "rtbf-auvio-be";

/// Public Gigya application identifier used by RTBF Auvio.
const RTBF_GIGYA_API_KEY: &str = "4_Ml_fJ47GnBAW6FrPzMxh0w";

/// RTBF Auvio Gigya login endpoint.
const RTBF_GIGYA_LOGIN_URL: &str = "https://login.auvio.rtbf.be/accounts.login";

/// RTBF Auvio Gigya JWT endpoint.
const RTBF_GIGYA_JWT_URL: &str = "https://login.auvio.rtbf.be/accounts.getJWT";

/// RTBF Auvio Red Bee entitlement API endpoint.
const REDBEE_API_ROOT: &str =
    "https://exposure.api.redbee.live:443/v2/customer/RTBF/businessunit/Auvio";

/// Default stream-kind identifier for the RTBF Auvio resolver.
const DEFAULT_STREAM_KIND: &str = "redbee-license-proxy";

/// Lifetime of cached RTBF Red Bee entitlement tokens.
const REDBEE_TOKEN_TTL: std::time::Duration = std::time::Duration::from_secs(55 * 60);

/// Timeout for RTBF provider-specific license forwarding.
const RTBF_LICENSE_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(25);

/// Process-wide cache of RTBF Auvio license proxy requests.
static REDBEE_LICENSE_CACHE: OnceLock<Mutex<HashMap<String, CachedRedbeeLicense>>> =
    OnceLock::new();

/// Authenticated RedBee session token.
#[derive(Clone)]
struct RedbeeSession {
    session_token: String,
}

/// Cached RTBF Auvio license request with its stream kind and expiry.
#[derive(Clone)]
struct CachedRedbeeLicense {
    license_url: String,
    stream_kind: String,
    expires_at: Instant,
}

/// RTBF Auvio implementation of the generic protected playback resolver contract.
pub(crate) struct RtbfAuvioResolver;

#[async_trait]
impl PlayerStreamResolver for RtbfAuvioResolver {
    fn source_id(&self) -> &'static str {
        RTBF_AUVIO_SERVICE_ID
    }

    fn resolver_ids(&self) -> &'static [&'static str] {
        &["rtbf-auvio-live", "rtbf-auvio-video"]
    }

    async fn get_stream(
        &self,
        scraper_agregator: &ScraperAgregator,
        credentials_store: &dyn CredentialsStore,
        request: PlayerStreamRequest<'_>,
        endpoints: &PlayerResolverEndpoints,
    ) -> Result<ResolvedPlayerStream> {
        let PlayerStreamRequest {
            resolver,
            target,
            service_parameters: _service_parameters,
            proxy_countries: _proxy_countries,
        } = request;
        match resolver.trim() {
            "rtbf-auvio-live" | "rtbf-auvio-video" => {
                resolve_redbee_stream(
                    scraper_agregator,
                    credentials_store,
                    target,
                    None,
                    endpoints,
                )
                .await
            }
            kind => bail!(
                "Unsupported player resolver `{}` for source `{}`.",
                kind,
                RTBF_AUVIO_SERVICE_ID
            ),
        }
    }

    async fn get_drm_license(
        &self,
        _scraper_agregator: &ScraperAgregator,
        stream_token: &str,
        body: &[u8],
    ) -> Result<ProxiedStreamResponse> {
        proxy_redbee_license_request(stream_token, body).await
    }
}

/// Resolves an RTBF Auvio asset into a protected RedBee player stream.
///
/// # Arguments
/// * `scraper_agregator` - Aggregator used to create the HTTP client.
/// * `credentials_store` - Store providing optional RTBF credentials.
/// * `asset_id` - RedBee asset identifier; surrounding whitespace is ignored.
/// * `stream_kind` - Optional caller-selected stream kind.
/// * `endpoints` - Public endpoint settings used to build proxy URLs.
///
/// # Errors
/// Returns an error when the identifier is empty, authentication or
/// entitlement retrieval fails, or no playable format is available.
///
/// # Returns
/// The resolved stream with proxied manifest, license and storyboard.
async fn resolve_redbee_stream(
    scraper_agregator: &ScraperAgregator,
    credentials_store: &dyn CredentialsStore,
    asset_id: &str,
    stream_kind: Option<String>,
    endpoints: &PlayerResolverEndpoints,
) -> Result<ResolvedPlayerStream> {
    let normalized_asset_id = asset_id.trim();
    if normalized_asset_id.is_empty() {
        bail!("Missing RTBF Auvio RedBee asset identifier.");
    }

    let http_client = scraper_agregator.create_http_client(Default::default());
    let session = authenticate_redbee(&http_client, credentials_store, normalized_asset_id).await?;
    let entitlement = fetch_entitlement(&http_client, normalized_asset_id, &session).await?;
    let selected_format = select_best_format(&entitlement)
        .with_context(|| entitlement_error_message(&entitlement))?;

    let stream_kind = normalize_redbee_stream_kind(stream_kind);
    let license_url = selected_format
        .license_url
        .map(|license_url| save_redbee_license_proxy_url(endpoints, &license_url, &stream_kind));
    let storyboard_vtt_url = extract_redbee_storyboard_vtt_url(&entitlement).map(|vtt_url| {
        proxied_redbee_media_url(&vtt_url, endpoints.http_proxy_public_path.as_deref())
    });

    Ok(ResolvedPlayerStream {
        stream_url: vec![proxied_redbee_media_url(
            &selected_format.media_locator,
            endpoints.http_proxy_public_path.as_deref(),
        )],
        manifest_type: Some(selected_format.manifest_type),
        license_url,
        license_headers: HashMap::new(),
        storyboard_vtt_url,
        ..Default::default()
    })
}

/// Extracts the first storyboard VTT URL from the entitlement sprites.
///
/// # Arguments
/// * `entitlement` - Decoded RTBF entitlement payload.
///
/// # Returns
/// The first non-empty VTT URL, or `None` when absent.
fn extract_redbee_storyboard_vtt_url(entitlement: &Value) -> Option<String> {
    entitlement
        .get("sprites")?
        .as_array()?
        .iter()
        .find_map(|sprite| {
            sprite
                .get("vtt")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
}

/// Prefixes an absolute media URL with the configured public proxy path.
///
/// # Arguments
/// * `media_locator` - Manifest or storyboard URL.
/// * `http_proxy_public_path` - Optional public HTTP proxy path.
///
/// # Returns
/// The proxied URL for absolute HTTP(S) locators, otherwise the trimmed
/// original locator.
fn proxied_redbee_media_url(media_locator: &str, http_proxy_public_path: Option<&str>) -> String {
    let normalized_media_locator = media_locator.trim();
    if normalized_media_locator.starts_with("http://")
        || normalized_media_locator.starts_with("https://")
    {
        if let Some(proxy_path) = http_proxy_public_path
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return format!(
                "{}/{}",
                proxy_path.trim_end_matches('/'),
                normalized_media_locator
            );
        }
    }

    normalized_media_locator.to_string()
}

/// Authenticates with RedBee, preferring a configured JWT when available.
///
/// # Arguments
/// * `http_client` - HTTP client used for authentication requests.
/// * `credentials_store` - Store providing optional RTBF credentials.
/// * `asset_id` - Asset identifier used for anonymous authentication.
///
/// # Errors
/// Returns an error when credentials cannot be read or authentication fails.
///
/// # Returns
/// The authenticated RedBee session.
async fn authenticate_redbee(
    http_client: &HttpClient,
    credentials_store: &dyn CredentialsStore,
    asset_id: &str,
) -> Result<RedbeeSession> {
    let credentials = credentials_store
        .get_credentials(RTBF_AUVIO_SERVICE_ID)
        .with_context(|| {
            format!(
                "Failed to load credentials for service `{}` from the configured store.",
                RTBF_AUVIO_SERVICE_ID
            )
        })?;

    if let Some(credentials) = credentials {
        let login = credentials.login.trim();
        let password = credentials.password.trim();

        if let Some(jwt) = credential_jwt(login, password) {
            return authenticate_with_jwt(http_client, jwt).await;
        }

        return authenticate_with_rtbf_credentials(http_client, login, password).await;
    }

    authenticate_anonymous(http_client, asset_id).await
}

/// Creates an anonymous RedBee session for an asset.
///
/// # Arguments
/// * `http_client` - HTTP client used for the request.
/// * `asset_id` - Asset identifier sent as a query parameter.
///
/// # Errors
/// Returns an error when the anonymous session cannot be created.
///
/// # Returns
/// The anonymous RedBee session.
async fn authenticate_anonymous(http_client: &HttpClient, asset_id: &str) -> Result<RedbeeSession> {
    let url = format!("{}/auth/anonymous?assetId={}", REDBEE_API_ROOT, asset_id);
    let body = auth_device_payload(None, None, None);
    request_redbee_session(http_client, &url, &body)
        .await
        .context("Failed to create an anonymous RTBF Auvio RedBee session.")
}

/// Authenticates RedBee with a Gigya JWT.
///
/// # Arguments
/// * `http_client` - HTTP client used for the request.
/// * `jwt` - Gigya identity token.
///
/// # Errors
/// Returns an error when RedBee rejects the JWT.
///
/// # Returns
/// The authenticated RedBee session.
async fn authenticate_with_jwt(http_client: &HttpClient, jwt: &str) -> Result<RedbeeSession> {
    let url = format!("{}/auth/gigyaLogin", REDBEE_API_ROOT);
    let body = auth_device_payload(None, None, Some(jwt));
    request_redbee_session(http_client, &url, &body)
        .await
        .context("Failed to authenticate RTBF Auvio RedBee with the configured JWT.")
}

/// Authenticates RedBee with an RTBF login and password.
///
/// # Arguments
/// * `http_client` - HTTP client used for the Gigya requests.
/// * `login` - RTBF account login.
/// * `password` - RTBF account password.
///
/// # Errors
/// Returns an error when the Gigya login, JWT exchange or RedBee
/// authentication fails.
///
/// # Returns
/// The authenticated RedBee session.
async fn authenticate_with_rtbf_credentials(
    http_client: &HttpClient,
    login: &str,
    password: &str,
) -> Result<RedbeeSession> {
    let login_token = fetch_rtbf_login_token(http_client, login, password).await?;
    let jwt = fetch_rtbf_jwt(http_client, &login_token).await?;

    authenticate_with_jwt(http_client, &jwt)
        .await
        .context("Failed to authenticate RTBF Auvio RedBee with the configured RTBF account.")
}

/// Fetches a Gigya login token for RTBF credentials.
///
/// # Arguments
/// * `http_client` - HTTP client used for the Gigya request.
/// * `login` - RTBF account login.
/// * `password` - RTBF account password.
///
/// # Errors
/// Returns an error when encoding or the request fails, Gigya rejects the
/// credentials, or the token is missing.
///
/// # Returns
/// The Gigya login cookie value.
async fn fetch_rtbf_login_token(
    http_client: &HttpClient,
    login: &str,
    password: &str,
) -> Result<String> {
    let body = serde_urlencoded::to_string([
        ("APIKey", RTBF_GIGYA_API_KEY),
        ("loginID", login),
        ("password", password),
    ])
    .context("Failed to serialize the RTBF Auvio login payload.")?;

    let response = http_client
        .get_json_for_request(
            http::Method::POST,
            RTBF_GIGYA_LOGIN_URL,
            &form_headers(),
            Some(&body),
        )
        .await
        .context("Failed to submit RTBF Auvio credentials to Gigya.")?;

    bail_on_rtbf_gigya_error(&response, "RTBF Auvio rejected the configured credentials.")?;
    read_json_string(
        &response,
        &["sessionInfo", "cookieValue"],
        "Missing RTBF Auvio Gigya login token.",
    )
}

/// Exchanges a Gigya login token for a JWT.
///
/// # Arguments
/// * `http_client` - HTTP client used for the Gigya request.
/// * `login_token` - Gigya login cookie value.
///
/// # Errors
/// Returns an error when encoding or the request fails, Gigya reports an
/// error, or the JWT is missing.
///
/// # Returns
/// The Gigya `id_token`.
async fn fetch_rtbf_jwt(http_client: &HttpClient, login_token: &str) -> Result<String> {
    let body =
        serde_urlencoded::to_string([("APIKey", RTBF_GIGYA_API_KEY), ("login_token", login_token)])
            .context("Failed to serialize the RTBF Auvio JWT payload.")?;

    let response = http_client
        .get_json_for_request(
            http::Method::POST,
            RTBF_GIGYA_JWT_URL,
            &form_headers(),
            Some(&body),
        )
        .await
        .context("Failed to fetch the RTBF Auvio Gigya JWT.")?;

    bail_on_rtbf_gigya_error(&response, "RTBF Auvio failed to issue a Gigya JWT.")?;
    read_json_string(
        &response,
        &["id_token"],
        "Missing RTBF Auvio Gigya id_token.",
    )
}

/// Requests a RedBee session and extracts its token.
///
/// # Arguments
/// * `http_client` - HTTP client used for the request.
/// * `url` - RedBee authentication URL.
/// * `body` - JSON authentication payload.
///
/// # Errors
/// Returns an error when the request fails, RedBee returns a `message`, or
/// the session token is missing.
///
/// # Returns
/// The authenticated RedBee session.
async fn request_redbee_session(
    http_client: &HttpClient,
    url: &str,
    body: &str,
) -> Result<RedbeeSession> {
    let response = http_client
        .get_json_for_request(http::Method::POST, url, &json_headers(), Some(body))
        .await?;

    if let Some(message) = response.get("message").and_then(Value::as_str) {
        bail!("RTBF Auvio RedBee authentication failed: {}", message);
    }

    Ok(RedbeeSession {
        session_token: read_json_string(
            &response,
            &["sessionToken"],
            "Missing RTBF Auvio RedBee session token.",
        )?,
    })
}

/// Fetches the playback entitlement for an asset.
///
/// # Arguments
/// * `http_client` - HTTP client used for the request.
/// * `asset_id` - RedBee asset identifier.
/// * `session` - Authenticated RedBee session.
///
/// # Errors
/// Returns an error when the request fails or the entitlement reports HTTP 403.
///
/// # Returns
/// The decoded entitlement payload.
async fn fetch_entitlement(
    http_client: &HttpClient,
    asset_id: &str,
    session: &RedbeeSession,
) -> Result<Value> {
    let url = format!("{}/entitlement/{}/play", REDBEE_API_ROOT, asset_id);
    let mut headers = json_headers();
    headers.insert(
        "authorization".to_string(),
        format!("Bearer {}", session.session_token),
    );

    let response = http_client
        .get_json_for_request(http::Method::GET, &url, &headers, None)
        .await?;

    if response.get("httpCode").and_then(Value::as_i64) == Some(403) {
        bail!("{}", entitlement_error_message(&response));
    }

    Ok(response)
}

/// Selected RTBF Auvio playback format.
struct SelectedRedbeeFormat {
    media_locator: String,
    manifest_type: String,
    license_url: Option<String>,
}

/// Selects a Widevine DASH format, then falls back to HLS.
///
/// # Arguments
/// * `entitlement` - Decoded RTBF entitlement payload.
///
/// # Returns
/// The preferred usable format, or `None` when none is available.
fn select_best_format(entitlement: &Value) -> Option<SelectedRedbeeFormat> {
    let formats = entitlement.get("formats")?.as_array()?;

    formats
        .iter()
        .find_map(|format| selected_format(format, "DASH", "mpd", true))
        .or_else(|| {
            formats
                .iter()
                .find_map(|format| selected_format(format, "HLS", "m3u8", false))
        })
        .or_else(|| {
            formats.iter().find_map(|format| {
                let format_name = format.get("format")?.as_str()?.trim();
                let manifest_type = match format_name {
                    "DASH" => "mpd",
                    "HLS" => "m3u8",
                    _ => return None,
                };
                selected_format(format, format_name, manifest_type, false)
            })
        })
}

/// Converts one entitlement format into a selected playback format.
///
/// # Arguments
/// * `format` - Entitlement format object.
/// * `expected_format` - Required value of the `format` field.
/// * `manifest_type` - Manifest type assigned to the result.
/// * `require_widevine_license` - Whether a Widevine license URL is mandatory.
///
/// # Returns
/// The selected format, or `None` when the type differs, the media locator is
/// missing, or the required license is absent.
fn selected_format(
    format: &Value,
    expected_format: &str,
    manifest_type: &str,
    require_widevine_license: bool,
) -> Option<SelectedRedbeeFormat> {
    if format.get("format")?.as_str()? != expected_format {
        return None;
    }

    let media_locator = format.get("mediaLocator")?.as_str()?.trim().to_string();
    if media_locator.is_empty() {
        return None;
    }

    let license_url = format
        .get("drm")
        .and_then(|drm| drm.get("com.widevine.alpha"))
        .and_then(|widevine| widevine.get("licenseServerUrl"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    if require_widevine_license && license_url.is_none() {
        return None;
    }

    Some(SelectedRedbeeFormat {
        media_locator,
        manifest_type: manifest_type.to_string(),
        license_url,
    })
}

/// Builds the user-facing entitlement error message.
///
/// # Arguments
/// * `entitlement` - Decoded RTBF entitlement payload.
///
/// # Returns
/// The server message, with a credentials hint when login is required.
fn entitlement_error_message(entitlement: &Value) -> String {
    let message = entitlement
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("Missing playable RTBF Auvio live format.");

    let requires_login = entitlement
        .get("actions")
        .and_then(Value::as_array)
        .map(|actions| {
            actions.iter().any(|action| {
                action
                    .get("type")
                    .and_then(Value::as_str)
                    .map(|value| value == "LOGIN")
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);

    if requires_login {
        format!(
            "{} Configure credentials for service `{}` to play this RTBF Auvio video.",
            message, RTBF_AUVIO_SERVICE_ID
        )
    } else {
        message.to_string()
    }
}

/// Builds the RedBee authentication device payload.
///
/// # Arguments
/// * `login` - Optional RTBF account login.
/// * `password` - Optional RTBF account password.
/// * `jwt` - Optional Gigya identity token.
///
/// # Returns
/// The serialized JSON device payload.
fn auth_device_payload(login: Option<&str>, password: Option<&str>, jwt: Option<&str>) -> String {
    let mut payload = json!({
        "device": {
            "deviceId": "arachnea",
            "name": "Browser",
            "type": "WEB"
        },
        "deviceId": "arachnea"
    });

    if let (Some(login), Some(password)) = (login, password) {
        payload["username"] = json!(login);
        payload["password"] = json!(password);
    }

    if let Some(jwt) = jwt {
        payload["jwt"] = json!(jwt);
    }

    payload.to_string()
}

/// Returns the configured login or password when it is a JWT.
///
/// # Arguments
/// * `login` - Configured login value.
/// * `password` - Configured password value.
///
/// # Returns
/// A borrowed JWT with an optional Bearer prefix stripped, or `None`.
fn credential_jwt<'a>(login: &'a str, password: &'a str) -> Option<&'a str> {
    let password = strip_bearer_prefix(password);
    let login = strip_bearer_prefix(login);

    if looks_like_jwt(password) {
        return Some(password);
    }

    if looks_like_jwt(login) {
        return Some(login);
    }

    None
}

/// Strips a case-variant `Bearer ` prefix and surrounding whitespace.
///
/// # Arguments
/// * `value` - Credential or token value.
///
/// # Returns
/// The trimmed value without a Bearer prefix.
fn strip_bearer_prefix(value: &str) -> &str {
    value
        .trim()
        .strip_prefix("Bearer ")
        .or_else(|| value.trim().strip_prefix("bearer "))
        .unwrap_or_else(|| value.trim())
}

/// Checks whether a value has the JWT shape used by RTBF.
///
/// # Arguments
/// * `value` - Candidate token.
///
/// # Returns
/// `true` for three dot-separated segments starting with `eyJ`.
fn looks_like_jwt(value: &str) -> bool {
    value.split('.').count() == 3 && value.starts_with("eyJ")
}

/// Normalizes an RTBF stream kind to the supported RedBee proxy kind.
///
/// # Arguments
/// * `stream_kind` - Optional caller-selected stream kind.
///
/// # Returns
/// The trimmed non-empty kind, or `DEFAULT_STREAM_KIND` when absent or blank.
fn normalize_redbee_stream_kind(stream_kind: Option<String>) -> String {
    stream_kind
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_STREAM_KIND.to_string())
}

/// Caches a RedBee license request and returns its public proxy URL.
///
/// # Arguments
/// * `endpoints` - Public endpoint settings used to build the proxy URL.
/// * `license_url` - Upstream Widevine license URL.
/// * `stream_kind` - Stream kind validated when the token is used.
///
/// # Returns
/// The public license proxy URL containing a generated token.
fn save_redbee_license_proxy_url(
    endpoints: &PlayerResolverEndpoints,
    license_url: &str,
    stream_kind: &str,
) -> String {
    let token_id = save_redbee_license_token(license_url, stream_kind);
    endpoints.drm_license_url(RTBF_AUVIO_SERVICE_ID, &token_id)
}

/// Stores a RedBee license request under a newly generated token.
///
/// # Arguments
/// * `license_url` - Upstream Widevine license URL.
/// * `stream_kind` - Stream kind validated when the token is used.
///
/// # Returns
/// The generated token. The entry is not stored when the cache mutex is
/// poisoned.
fn save_redbee_license_token(license_url: &str, stream_kind: &str) -> String {
    let token_id: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(crate::PLAYER_PROXY_TOKEN_LENGTH)
        .map(char::from)
        .collect();

    let cache = REDBEE_LICENSE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache_guard) = cache.lock() else {
        return token_id;
    };

    let now = Instant::now();
    cache_guard.retain(|_, entry| entry.expires_at > now);
    cache_guard.insert(
        token_id.clone(),
        CachedRedbeeLicense {
            license_url: license_url.to_string(),
            stream_kind: stream_kind.to_string(),
            expires_at: now + REDBEE_TOKEN_TTL,
        },
    );

    token_id
}

/// Loads a non-expired RedBee license request, pruning expired entries.
///
/// # Arguments
/// * `token_id` - Public token used as the cache key.
///
/// # Returns
/// The cached license request, or `None` when absent, expired, or the cache
/// mutex is poisoned.
fn load_cached_redbee_license(token_id: &str) -> Option<CachedRedbeeLicense> {
    let cache = REDBEE_LICENSE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache_guard) = cache.lock() else {
        return None;
    };

    let now = Instant::now();
    cache_guard.retain(|_, entry| entry.expires_at > now);
    cache_guard.get(token_id).cloned()
}

/// Proxies a Widevine challenge using a cached RedBee license request.
///
/// # Arguments
/// * `token_id` - Public token identifying the cached license request.
/// * `challenge_body` - Raw Widevine challenge bytes.
///
/// # Errors
/// Returns an error when the token is empty, expired or unsupported, the
/// client cannot be built, or the upstream request fails.
///
/// # Returns
/// The upstream license response with a `no-store` cache policy.
async fn proxy_redbee_license_request(
    token_id: &str,
    challenge_body: &[u8],
) -> Result<ProxiedStreamResponse> {
    let normalized_token_id = token_id.trim();
    if normalized_token_id.is_empty() {
        bail!("Missing RTBF Auvio license token identifier.");
    }

    let cached_license = load_cached_redbee_license(normalized_token_id)
        .context("Expired or missing RTBF Auvio license token.")?;

    if cached_license.stream_kind != DEFAULT_STREAM_KIND {
        bail!(
            "Unsupported RTBF Auvio stream kind `{}`.",
            cached_license.stream_kind
        );
    }

    let response = rquest::Client::builder()
        .user_agent(crate::PLAYER_DEFAULT_USER_AGENT)
        .timeout(RTBF_LICENSE_REQUEST_TIMEOUT)
        .build()
        .context("Failed to build the RTBF Auvio license proxy client.")?
        .post(cached_license.license_url.trim())
        .header(
            CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        )
        .body(challenge_body.to_vec())
        .send()
        .await
        .context("Failed to call the RTBF Auvio RedBee Widevine license server.")?;

    let status = response.status();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let body = response
        .bytes()
        .await
        .context("Failed to read the RTBF Auvio RedBee license response.")?
        .to_vec();

    if !status.is_success() {
        let details = String::from_utf8(body)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("HTTP {}", status));
        bail!("RTBF Auvio RedBee license request failed: {}", details);
    }

    Ok(ProxiedStreamResponse {
        body,
        content_type,
        headers: HashMap::from([("cache-control".to_string(), "no-store".to_string())]),
    })
}

/// Builds JSON headers for RedBee API requests.
///
/// # Returns
/// JSON acceptance and content-type headers.
fn json_headers() -> HashMap<String, String> {
    HashMap::from([
        ("accept".to_string(), "application/json".to_string()),
        ("content-type".to_string(), "application/json".to_string()),
    ])
}

/// Builds form headers for Gigya requests.
///
/// # Returns
/// JSON acceptance and form content-type headers.
fn form_headers() -> HashMap<String, String> {
    HashMap::from([
        ("accept".to_string(), "application/json".to_string()),
        (
            "content-type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        ),
    ])
}

/// Fails when a Gigya response contains an error message.
///
/// # Arguments
/// * `payload` - Decoded Gigya response.
/// * `fallback_message` - Context prepended to the server message.
///
/// # Errors
/// Returns an error when `errorMessage` is present and non-empty.
fn bail_on_rtbf_gigya_error(payload: &Value, fallback_message: &str) -> Result<()> {
    if let Some(message) = payload
        .get("errorMessage")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        bail!("{} {}", fallback_message, message);
    }

    Ok(())
}

/// Reads a non-empty string from a nested JSON path.
///
/// # Arguments
/// * `payload` - JSON value to traverse.
/// * `path` - Object keys applied successively.
/// * `error_message` - Message used when a key or string is missing.
///
/// # Errors
/// Returns an error when traversal fails or the target is missing, blank or
/// not a string.
///
/// # Returns
/// The trimmed string value.
fn read_json_string(payload: &Value, path: &[&str], error_message: &str) -> Result<String> {
    let mut current = payload;
    for segment in path {
        current = current
            .get(*segment)
            .with_context(|| error_message.to_string())?;
    }

    current
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .with_context(|| error_message.to_string())
}
