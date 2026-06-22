use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use rand::{distr::Alphanumeric, Rng};
use rquest::header::{HeaderValue, CONTENT_TYPE};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use arachnea_core::persistence::CredentialsStore;
use arachnea_scrapyfy::{HttpClient, ScraperAgregator, ScraperQueryCollectionParameter};

use crate::services::player_resolver::{
    PlayerStreamResolver, ProxiedStreamResponse, ResolvedPlayerStream,
};

const RTBF_AUVIO_SERVICE_ID: &str = "rtbf-auvio-be";
const RTBF_GIGYA_API_KEY: &str = "4_Ml_fJ47GnBAW6FrPzMxh0w";
const RTBF_GIGYA_LOGIN_URL: &str = "https://login.auvio.rtbf.be/accounts.login";
const RTBF_GIGYA_JWT_URL: &str = "https://login.auvio.rtbf.be/accounts.getJWT";
const REDBEE_API_ROOT: &str =
    "https://exposure.api.redbee.live:443/v2/customer/RTBF/businessunit/Auvio";
const STREAM_PROXY_PATH_PREFIX: &str = "/api/get_stream/";
const DEFAULT_STREAM_KIND: &str = "redbee-license-proxy";
const REDBEE_TOKEN_TTL: Duration = Duration::from_secs(55 * 60);

static REDBEE_LICENSE_CACHE: OnceLock<Mutex<HashMap<String, CachedRedbeeLicense>>> =
    OnceLock::new();

#[derive(Clone)]
struct RedbeeSession {
    session_token: String,
}

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

    async fn resolve_player_stream(
        &self,
        scraper_agregator: &ScraperAgregator,
        credentials_store: &dyn CredentialsStore,
        resolver_kind: &str,
        resolver_target: &str,
        resolver_stream_kind: Option<String>,
        _service_parameters: &[ScraperQueryCollectionParameter],
    ) -> Result<ResolvedPlayerStream> {
        match resolver_kind.trim() {
            "rtbf-auvio-live" | "rtbf-auvio-video" => {
                resolve_redbee_stream(
                    scraper_agregator,
                    credentials_store,
                    resolver_target,
                    resolver_stream_kind,
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

    async fn get_stream(
        &self,
        _scraper_agregator: &ScraperAgregator,
        stream_token: &str,
        body: &[u8],
    ) -> Result<ProxiedStreamResponse> {
        proxy_redbee_license_request(stream_token, body).await
    }
}

async fn resolve_redbee_stream(
    scraper_agregator: &ScraperAgregator,
    credentials_store: &dyn CredentialsStore,
    asset_id: &str,
    stream_kind: Option<String>,
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
        .map(|license_url| save_redbee_license_proxy_url(&license_url, &stream_kind));

    Ok(ResolvedPlayerStream {
        stream_url: selected_format.media_locator,
        manifest_type: selected_format.manifest_type,
        license_url,
        license_headers: HashMap::new(),
    })
}

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

async fn authenticate_anonymous(http_client: &HttpClient, asset_id: &str) -> Result<RedbeeSession> {
    let url = format!("{}/auth/anonymous?assetId={}", REDBEE_API_ROOT, asset_id);
    let body = auth_device_payload(None, None, None);
    request_redbee_session(http_client, &url, &body)
        .await
        .context("Failed to create an anonymous RTBF Auvio RedBee session.")
}

async fn authenticate_with_jwt(http_client: &HttpClient, jwt: &str) -> Result<RedbeeSession> {
    let url = format!("{}/auth/gigyaLogin", REDBEE_API_ROOT);
    let body = auth_device_payload(None, None, Some(jwt));
    request_redbee_session(http_client, &url, &body)
        .await
        .context("Failed to authenticate RTBF Auvio RedBee with the configured JWT.")
}

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

struct SelectedRedbeeFormat {
    media_locator: String,
    manifest_type: String,
    license_url: Option<String>,
}

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

fn strip_bearer_prefix(value: &str) -> &str {
    value
        .trim()
        .strip_prefix("Bearer ")
        .or_else(|| value.trim().strip_prefix("bearer "))
        .unwrap_or_else(|| value.trim())
}

fn looks_like_jwt(value: &str) -> bool {
    value.split('.').count() == 3 && value.starts_with("eyJ")
}

fn normalize_redbee_stream_kind(stream_kind: Option<String>) -> String {
    stream_kind
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_STREAM_KIND.to_string())
}

fn save_redbee_license_proxy_url(license_url: &str, stream_kind: &str) -> String {
    let token_id = save_redbee_license_token(license_url, stream_kind);
    format!(
        "{}{}/{}",
        STREAM_PROXY_PATH_PREFIX,
        RTBF_AUVIO_SERVICE_ID,
        token_id.trim()
    )
}

fn save_redbee_license_token(license_url: &str, stream_kind: &str) -> String {
    let token_id: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(32)
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

fn load_cached_redbee_license(token_id: &str) -> Option<CachedRedbeeLicense> {
    let cache = REDBEE_LICENSE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache_guard) = cache.lock() else {
        return None;
    };

    let now = Instant::now();
    cache_guard.retain(|_, entry| entry.expires_at > now);
    cache_guard.get(token_id).cloned()
}

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
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:148.0) Gecko/20100101 Firefox/148.0",
        )
        .timeout(Duration::from_secs(25))
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

fn json_headers() -> HashMap<String, String> {
    HashMap::from([
        ("accept".to_string(), "application/json".to_string()),
        ("content-type".to_string(), "application/json".to_string()),
    ])
}

fn form_headers() -> HashMap<String, String> {
    HashMap::from([
        ("accept".to_string(), "application/json".to_string()),
        (
            "content-type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        ),
    ])
}

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
