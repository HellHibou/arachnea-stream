use anyhow::{bail, Context, Result};
use arachnea_proxy::http::proxy_service::proxied_url;
use async_trait::async_trait;
use rand::{distr::Alphanumeric, Rng};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use url::Url;
use xxhash_rust::xxh3::xxh3_64;

use arachnea_core::persistence::CredentialsStore;
use arachnea_scrapyfy::{HttpClient, ScraperAgregator, ScraperQueryCollectionParameter};

use crate::services::player_resolver::{
    PlayerResolverEndpoints, PlayerStreamResolver, ProxiedStreamResponse, ResolvedPlayerStream,
};

const SERVICE_ID: &str = "antennereunion-fr";
const API_BASE_URL: &str = "https://api.antennereunion-production.eu-west-3.alphanetworks.tv";
const PROXY_BASE_URL: &str = "https://proxies.antennereunion-production.eu-west-3.alphanetworks.tv";
const IDENTITY_KEY: &str = "HadCogoctandsxEDEsdufOcagidOad";
const LANGUAGE_ID: &str = "fra";
const SESSION_TTL: Duration = Duration::from_secs(55 * 60);
const LICENSE_TTL: Duration = Duration::from_secs(15 * 60);
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:148.0) Gecko/20100101 Firefox/148.0";

static SESSION_CACHE: OnceLock<Mutex<HashMap<String, CachedSession>>> = OnceLock::new();
static LICENSE_CACHE: OnceLock<Mutex<HashMap<String, CachedLicense>>> = OnceLock::new();
static LOGIN_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

#[derive(Clone)]
struct Session {
    customer_token: String,
    device_token: String,
    profile_token: String,
}

struct CachedSession {
    session: Session,
    expires_at: Instant,
}

#[derive(Clone)]
struct CachedLicense {
    license_url: String,
    headers: HashMap<String, String>,
    expires_at: Instant,
}

/// Antenne Réunion implementation of the authenticated Tucano VOD resolver.
pub(crate) struct AntenneReunionResolver;

#[async_trait]
impl PlayerStreamResolver for AntenneReunionResolver {
    fn source_id(&self) -> &'static str {
        SERVICE_ID
    }

    fn resolver_ids(&self) -> &'static [&'static str] {
        &["antennereunion-video"]
    }

    async fn get_stream(
        &self,
        scraper_agregator: &ScraperAgregator,
        credentials_store: &dyn CredentialsStore,
        resolver: &str,
        target: &str,
        _service_parameters: &[ScraperQueryCollectionParameter],
        endpoints: &PlayerResolverEndpoints,
    ) -> Result<ResolvedPlayerStream> {
        if resolver.trim() != "antennereunion-video" {
            bail!("Unsupported player resolver `{resolver}` for source `{SERVICE_ID}`.");
        }

        resolve_vod_stream(scraper_agregator, credentials_store, target, endpoints).await
    }

    async fn get_drm_license(
        &self,
        scraper_agregator: &ScraperAgregator,
        stream_token: &str,
        body: &[u8],
    ) -> Result<ProxiedStreamResponse> {
        proxy_license_request(scraper_agregator, stream_token, body).await
    }
}

async fn resolve_vod_stream(
    scraper_agregator: &ScraperAgregator,
    credentials_store: &dyn CredentialsStore,
    asset_id: &str,
    endpoints: &PlayerResolverEndpoints,
) -> Result<ResolvedPlayerStream> {
    let asset_id = asset_id.trim();
    if asset_id.is_empty() {
        bail!("Missing Antenne Réunion asset identifier.");
    }

    let http_client = scraper_agregator.create_http_client(Default::default());
    let (login, password) = load_credentials(credentials_store)?;
    let session = get_or_login_session(&http_client, &login, &password).await?;
    let headers = playback_headers(&session);
    let stream = post_proxy_form(
        &http_client,
        "readAsset",
        &headers,
        &[
            ("idAsset", asset_id.to_string()),
            ("idAudioLang", "fra".to_string()),
            ("idSubtitleLang", "non".to_string()),
            ("adsMacro", ads_macro(asset_id)?),
        ],
    )
    .await?;
    bail_on_read_asset_error(&stream)?;
    let stream_url =
        read_json_string(&stream, &["url"], "Missing Antenne Réunion VOD stream URL.")?;
    let stream_url = http_client
        .resolve_final_url_for_request(http::Method::GET, &stream_url, &HashMap::new(), None)
        .await
        .context("Failed to initialize the Antenne Réunion Broadpeak playback session.")?;
    let license = post_proxy_form(
        &http_client,
        "getVodLicense",
        &headers,
        &[
            ("idAsset", asset_id.to_string()),
            ("idAudioLang", "fra".to_string()),
            ("idSubtitleLang", "non".to_string()),
        ],
    )
    .await?;
    let license_url = find_first_json_value(&license, "licParam")
        .map(parse_license_parameter)
        .transpose()?
        .flatten()
        .map(|license| save_license(endpoints, &license));

    Ok(ResolvedPlayerStream {
        stream_url: vec![proxied_url(
            &stream_url,
            endpoints.http_proxy_public_path.as_deref(),
            None,
            &[],
            &[],
        )],
        manifest_type: Some(manifest_type(&stream_url).to_string()),
        license_url,
        license_headers: HashMap::new(),
        ..Default::default()
    })
}

async fn get_or_login_session(
    http_client: &HttpClient,
    login: &str,
    password: &str,
) -> Result<Session> {
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

    let token = post_form(
        http_client,
        &format!("{API_BASE_URL}/oauth/token"),
        &HashMap::from([(
            "content-type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        )]),
        &[
            ("username", login.to_string()),
            ("password", password.to_string()),
            ("client_id", IDENTITY_KEY.to_string()),
            ("grant_type", "password".to_string()),
            ("id_device", stable_device_id(login)),
            ("languageId", LANGUAGE_ID.to_string()),
        ],
        "Failed to authenticate with Antenne Réunion.",
    )
    .await?;
    let mut customer_token = read_json_string(
        &token,
        &["access_token"],
        "Missing Antenne Réunion access token.",
    )?;
    let device_token = read_json_string(
        &token,
        &["refresh_token"],
        "Missing Antenne Réunion device refresh token.",
    )?;
    let initial_customer_headers = customer_headers(&customer_token, &device_token);
    let profiles = http_client
        .get_json_for_request(
            http::Method::GET,
            &format!("{PROXY_BASE_URL}/crm/profile?languageId={LANGUAGE_ID}"),
            &initial_customer_headers,
            None,
        )
        .await
        .context("Failed to list Antenne Réunion profiles.")?;
    let profiles = unwrap_api_result(profiles, "Failed to list Antenne Réunion profiles.")?;
    if let Some(new_auth_token) = profiles
        .get("newAuthToken")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        customer_token = new_auth_token.to_string();
    }
    let profile_id = first_profile_id(&profiles)?;
    let active_profile_headers = customer_headers(&customer_token, &device_token);
    let active_profile = post_form(
        http_client,
        &format!("{PROXY_BASE_URL}/crm/profile/active"),
        &active_profile_headers,
        &[
            ("idProfile", profile_id),
            ("languageId", LANGUAGE_ID.to_string()),
        ],
        "Failed to activate the Antenne Réunion profile.",
    )
    .await?;
    let profile_token = read_json_string(
        &active_profile,
        &["profileToken"],
        "Missing active Antenne Réunion profile token.",
    )?;
    let session = Session {
        customer_token,
        device_token,
        profile_token,
    };
    save_cached_session(login, &session);
    Ok(session)
}

fn first_profile_id(payload: &Value) -> Result<String> {
    let profile = payload
        .get("profiles")
        .and_then(Value::as_array)
        .and_then(|profiles| profiles.first())
        .context("Antenne Réunion did not return an account profile.")?;
    find_first_json_string(profile, "id")
        .or_else(|| find_first_json_string(profile, "idProfile"))
        .or_else(|| find_first_json_string(profile, "profileId"))
        .with_context(|| {
            format!(
                "Antenne Réunion profile does not expose an identifier (profile shape: {}).",
                json_shape(payload)
            )
        })
}

/// Finds the first non-empty scalar stored under `key` in a JSON response.
///
/// Tucano profile identifiers can be named differently by deployment and can
/// be JSON strings or numbers.
fn find_first_json_string(payload: &Value, key: &str) -> Option<String> {
    match payload {
        Value::Object(object) => {
            if let Some(value) = object.get(key) {
                if let Some(value) = value
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    return Some(value.to_string());
                }
                if let Some(value) = value.as_i64() {
                    return Some(value.to_string());
                }
                if let Some(value) = value.as_u64() {
                    return Some(value.to_string());
                }
            }

            object
                .values()
                .find_map(|value| find_first_json_string(value, key))
        }
        Value::Array(values) => values
            .iter()
            .find_map(|value| find_first_json_string(value, key)),
        _ => None,
    }
}

/// Finds the first JSON value stored under `key` in a nested API response.
fn find_first_json_value<'a>(payload: &'a Value, key: &str) -> Option<&'a Value> {
    match payload {
        Value::Object(object) => object.get(key).or_else(|| {
            object
                .values()
                .find_map(|value| find_first_json_value(value, key))
        }),
        Value::Array(values) => values
            .iter()
            .find_map(|value| find_first_json_value(value, key)),
        _ => None,
    }
}

/// Describes the JSON container shape without including sensitive values.
fn json_shape(payload: &Value) -> String {
    match payload {
        Value::Object(object) => {
            let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
            keys.sort_unstable();
            format!("object({})", keys.join(", "))
        }
        Value::Array(values) => format!("array(len={})", values.len()),
        Value::Null => "null".to_string(),
        Value::Bool(_) => "boolean".to_string(),
        Value::Number(_) => "number".to_string(),
        Value::String(_) => "string".to_string(),
    }
}

async fn post_proxy_form(
    http_client: &HttpClient,
    endpoint: &str,
    headers: &HashMap<String, String>,
    fields: &[(&str, String)],
) -> Result<Value> {
    post_form(
        http_client,
        &format!("{PROXY_BASE_URL}/proxy/{endpoint}"),
        headers,
        fields,
        &format!("Failed to call Antenne Réunion `{endpoint}` playback endpoint."),
    )
    .await
}

async fn post_form(
    http_client: &HttpClient,
    url: &str,
    headers: &HashMap<String, String>,
    fields: &[(&str, String)],
    error_context: &str,
) -> Result<Value> {
    let body =
        serde_urlencoded::to_string(fields).context("Failed to encode Antenne Réunion request.")?;
    let payload = http_client
        .get_json_for_request(http::Method::POST, url, headers, Some(&body))
        .await
        .with_context(|| error_context.to_string())?;
    unwrap_api_result(payload, error_context)
}

fn unwrap_api_result(payload: Value, error_context: &str) -> Result<Value> {
    if payload.get("status").and_then(Value::as_bool) == Some(false) {
        let message = payload
            .pointer("/error/message")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("unknown API error");
        bail!("{error_context}: {message}");
    }
    Ok(payload.get("result").cloned().unwrap_or(payload))
}

/// Serializes advertising metadata exactly like the official web player.
fn ads_macro(asset_id: &str) -> Result<String> {
    serde_urlencoded::to_string([
        (
            "page_url",
            format!("https://www.antennereunion.fr/content/asset-{asset_id}"),
        ),
        ("device_ua", USER_AGENT.to_string()),
    ])
    .context("Failed to encode Antenne Réunion advertising metadata.")
}

/// Surfaces a playback entitlement error before attempting to read its stream URL.
fn bail_on_read_asset_error(payload: &Value) -> Result<()> {
    let Some(error_code) = payload.get("errorCode") else {
        return Ok(());
    };
    let error_code = error_code
        .as_i64()
        .map(|value| value.to_string())
        .or_else(|| {
            error_code
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".to_string());
    let message = payload
        .get("message")
        .or_else(|| payload.pointer("/error/message"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!(": {value}"))
        .unwrap_or_default();
    bail!("Antenne Réunion readAsset was refused with error code {error_code}{message}");
}

fn playback_headers(session: &Session) -> HashMap<String, String> {
    HashMap::from([
        (
            "X-AN-WebService-IdentityKey".to_string(),
            IDENTITY_KEY.to_string(),
        ),
        (
            "X-AN-WebService-CustomerAuthToken".to_string(),
            session.customer_token.clone(),
        ),
        (
            "X-AN-WebService-DeviceAuthToken".to_string(),
            session.device_token.clone(),
        ),
        (
            "X-AN-WebService-ProfileToken".to_string(),
            session.profile_token.clone(),
        ),
        (
            "content-type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        ),
    ])
}

/// Builds the customer/device headers used by the official Tucano `BaseApi` client.
fn customer_headers(customer_token: &str, device_token: &str) -> HashMap<String, String> {
    HashMap::from([
        (
            "X-AN-WebService-IdentityKey".to_string(),
            IDENTITY_KEY.to_string(),
        ),
        (
            "X-AN-WebService-CustomerAuthToken".to_string(),
            customer_token.to_string(),
        ),
        (
            "X-AN-WebService-DeviceAuthToken".to_string(),
            device_token.to_string(),
        ),
        (
            "content-type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        ),
    ])
}

struct LicenseParameter {
    license_url: String,
    headers: HashMap<String, String>,
}

fn parse_license_parameter(value: &Value) -> Result<Option<LicenseParameter>> {
    let payload = match value {
        Value::String(query) => {
            let fields: HashMap<String, String> = serde_urlencoded::from_str(query)
                .context("Invalid string Antenne Réunion DRM license parameter.")?;
            Value::Object(
                fields
                    .into_iter()
                    .map(|(key, value)| (key, Value::String(value)))
                    .collect(),
            )
        }
        Value::Object(_) => value.clone(),
        Value::Null => return Ok(None),
        _ => bail!("Invalid Antenne Réunion DRM license parameter."),
    };
    let object = payload
        .as_object()
        .context("Invalid Antenne Réunion DRM license object.")?;
    let mut license_url = object_string(object, "url");
    if license_url.is_empty() {
        return Ok(None);
    }
    license_url = license_url.replacen("skd://", "https://", 1);
    let mut vendor = object_string(object, "vendor").to_ascii_lowercase();
    if license_url.contains("vudrm.tech") {
        vendor = "vualto".to_string();
    }
    let session_token = object_string(object, "sessionToken");
    let token = if session_token.is_empty() {
        object_string(object, "token")
    } else {
        session_token
    };
    let headers = match vendor.as_str() {
        "vualto" if !token.is_empty() => {
            let separator = if license_url.contains('?') { "&" } else { "?" };
            license_url = format!(
                "{license_url}{separator}token={}",
                urlencoding::encode(&token)
            );
            HashMap::new()
        }
        "castlabs" | "drmtoday" if !token.is_empty() => HashMap::from([
            ("x-dt-auth-token".to_string(), token),
            (
                "content-type".to_string(),
                "application/octet-stream".to_string(),
            ),
        ]),
        _ if !token.is_empty() => {
            HashMap::from([("authorization".to_string(), format!("Bearer {token}"))])
        }
        _ => HashMap::new(),
    };
    Ok(Some(LicenseParameter {
        license_url,
        headers,
    }))
}

fn object_string(object: &Map<String, Value>, key: &str) -> String {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn save_license(endpoints: &PlayerResolverEndpoints, license: &LicenseParameter) -> String {
    let token: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();
    let cache = LICENSE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut cache) = cache.lock() {
        let now = Instant::now();
        cache.retain(|_, entry| entry.expires_at > now);
        cache.insert(
            token.clone(),
            CachedLicense {
                license_url: license.license_url.clone(),
                headers: license.headers.clone(),
                expires_at: now + LICENSE_TTL,
            },
        );
    }
    endpoints.drm_license_url(SERVICE_ID, &token)
}

async fn proxy_license_request(
    scraper_agregator: &ScraperAgregator,
    token: &str,
    challenge_body: &[u8],
) -> Result<ProxiedStreamResponse> {
    let cached =
        load_cached_license(token).context("Expired or missing Antenne Réunion license token.")?;
    let mut headers = cached.headers;
    headers.insert(
        "content-type".to_string(),
        "application/octet-stream".to_string(),
    );
    let response = scraper_agregator
        .create_http_client(Default::default())
        .send_bytes_for_request(
            http::Method::POST,
            &cached.license_url,
            &headers,
            Some(challenge_body.to_vec()),
        )
        .await
        .context("Failed to call the Antenne Réunion Widevine license server.")?;
    let status = response.status();
    let content_type = response
        .headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let body = response
        .bytes()
        .await
        .context("Failed to read the Antenne Réunion license response.")?
        .to_vec();
    if !status.is_success() {
        let details = String::from_utf8(body)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("HTTP {status}"));
        bail!("Antenne Réunion license request failed: {details}");
    }
    Ok(ProxiedStreamResponse {
        body,
        content_type,
        headers: HashMap::from([("cache-control".to_string(), "no-store".to_string())]),
    })
}

fn load_credentials(credentials_store: &dyn CredentialsStore) -> Result<(String, String)> {
    let credentials = credentials_store
        .get_credentials(SERVICE_ID)
        .with_context(|| format!("Failed to load credentials for service `{SERVICE_ID}`."))?
        .with_context(|| format!("Missing credentials for service `{SERVICE_ID}`."))?;
    Ok((credentials.login, credentials.password))
}

fn load_cached_session(login: &str) -> Option<Session> {
    let cache = SESSION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache) = cache.lock() else {
        return None;
    };
    let now = Instant::now();
    cache.retain(|_, entry| entry.expires_at > now);
    cache.get(login).map(|entry| entry.session.clone())
}

fn save_cached_session(login: &str, session: &Session) {
    let cache = SESSION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache) = cache.lock() else {
        return;
    };
    cache.insert(
        login.to_string(),
        CachedSession {
            session: session.clone(),
            expires_at: Instant::now() + SESSION_TTL,
        },
    );
}

fn load_cached_license(token: &str) -> Option<CachedLicense> {
    let cache = LICENSE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache) = cache.lock() else {
        return None;
    };
    let now = Instant::now();
    cache.retain(|_, entry| entry.expires_at > now);
    cache.get(token.trim()).cloned()
}

fn stable_device_id(login: &str) -> String {
    let first = xxh3_64(login.trim().as_bytes());
    let second = xxh3_64(format!("{login}:antennereunion").as_bytes());
    format!(
        "{:08x}-{:04x}-4{:03x}-8{:03x}-{:012x}",
        first as u32,
        (second >> 48) as u16,
        ((first >> 16) & 0x0fff) as u16,
        ((first >> 32) & 0x0fff) as u16,
        second & 0x0000_ffff_ffff_ffff,
    )
}

fn manifest_type(stream_url: &str) -> &'static str {
    Url::parse(stream_url)
        .ok()
        .map(|url| url.path().to_ascii_lowercase())
        .filter(|path| path.ends_with(".mpd"))
        .map(|_| "mpd")
        .unwrap_or("m3u8")
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
