use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use rand::{distr::Alphanumeric, Rng};
use rquest::{
    header::{HeaderValue, CONTENT_TYPE},
    Url,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use arachnea_core::persistence::CredentialsStore;
use arachnea_scrapyfy::{HttpClient, ScraperAgregator, ScraperQueryCollectionParameter};

use crate::services::player_resolver::{
    PlayerResolverEndpoints, PlayerStreamResolver, ProxiedStreamResponse, ResolvedPlayerStream,
};

const FRANCETV_SERVICE_ID: &str = "francetv";
const FRANCETV_BASE_URL: &str = "https://www.france.tv";
const FRANCETV_VIDEO_INFO_URL_TEMPLATE: &str = "https://k7.ftven.fr/videos/{}";
const FRANCETV_DEFAULT_TOKEN_URL: &str = "https://hdfauth.ftven.fr/esi/TA";
const FRANCETV_WIDEVINE_LICENSE_URL: &str =
    "https://api-drm.ftven.fr/v1/wvls/contentlicenseservice/v1/licenses/";
const FRANCETV_PROXY_STREAM_KIND: &str = "francetv-license-proxy";
const STREAM_PROXY_PATH_PREFIX: &str = "/api/get_stream/";
const FRANCETV_LICENSE_TTL: Duration = Duration::from_secs(15 * 60);
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:148.0) Gecko/20100101 Firefox/148.0";

static FRANCETV_LICENSE_CACHE: OnceLock<Mutex<HashMap<String, CachedFrancetvLicense>>> =
    OnceLock::new();

#[derive(Clone)]
struct CachedFrancetvLicense {
    authorization_token: String,
    license_url: String,
    stream_kind: String,
    expires_at: Instant,
}

/// FranceTV implementation of the generic protected playback resolver contract.
pub(crate) struct FrancetvResolver;

#[async_trait]
impl PlayerStreamResolver for FrancetvResolver {
    fn source_id(&self) -> &'static str {
        FRANCETV_SERVICE_ID
    }

    async fn resolve_player_stream(
        &self,
        scraper_agregator: &ScraperAgregator,
        _credentials_store: &dyn CredentialsStore,
        resolver_kind: &str,
        resolver_target: &str,
        resolver_stream_kind: Option<String>,
        service_parameters: &[ScraperQueryCollectionParameter],
        _endpoints: &PlayerResolverEndpoints,
    ) -> Result<ResolvedPlayerStream> {
        match resolver_kind.trim() {
            "francetv-video" => {
                resolve_francetv_stream(
                    scraper_agregator,
                    resolver_target,
                    resolver_stream_kind,
                    service_parameters,
                    false,
                )
                .await
            }
            "francetv-live" => {
                resolve_francetv_stream(
                    scraper_agregator,
                    resolver_target,
                    resolver_stream_kind,
                    service_parameters,
                    true,
                )
                .await
            }
            kind => bail!(
                "Unsupported player resolver `{}` for source `{}`.",
                kind,
                FRANCETV_SERVICE_ID
            ),
        }
    }

    async fn get_stream(
        &self,
        _scraper_agregator: &ScraperAgregator,
        stream_token: &str,
        body: &[u8],
    ) -> Result<ProxiedStreamResponse> {
        proxy_francetv_license_request(stream_token, body).await
    }
}

async fn resolve_francetv_stream(
    scraper_agregator: &ScraperAgregator,
    media_id: &str,
    stream_kind: Option<String>,
    service_parameters: &[ScraperQueryCollectionParameter],
    is_live: bool,
) -> Result<ResolvedPlayerStream> {
    let normalized_media_id = media_id.trim();
    if normalized_media_id.is_empty() {
        bail!("Missing FranceTV media identifier.");
    }

    let country_code =
        parameter_value(service_parameters, "country_code").unwrap_or_else(|| "FR".to_string());
    let http_client = scraper_agregator.create_http_client(Default::default());
    let media_info =
        fetch_media_info(&http_client, normalized_media_id, country_code.trim()).await?;
    let video = media_info
        .get("video")
        .context("Missing FranceTV `video` payload in K7 response.")?;

    let raw_manifest_url = read_json_string(
        video,
        &["url"],
        "Missing FranceTV media manifest URL in K7 response.",
    )?;
    let format = video
        .get("format")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let drm_enabled = video.get("drm").and_then(Value::as_bool).unwrap_or(false);

    let manifest_token_url = if drm_enabled && !is_live {
        FRANCETV_DEFAULT_TOKEN_URL.to_string()
    } else {
        token_url_from_video(video, false)
    };
    let stream_url =
        fetch_signed_manifest_url(&http_client, &manifest_token_url, &raw_manifest_url).await?;
    let manifest_type = manifest_type_from_format_or_url(format, &stream_url);

    if !drm_enabled {
        return Ok(ResolvedPlayerStream {
            stream_url,
            manifest_type,
            license_url: None,
            license_headers: HashMap::new(),
        });
    }

    let drm_token_url = token_url_from_video(video, true);
    let authorization_token =
        fetch_widevine_authorization_token(&http_client, &drm_token_url, normalized_media_id)
            .await?;
    let stream_kind = normalize_francetv_stream_kind(stream_kind);
    let license_url = save_francetv_license_proxy_url(
        &authorization_token,
        &stream_kind,
        FRANCETV_WIDEVINE_LICENSE_URL,
    );

    Ok(ResolvedPlayerStream {
        stream_url,
        manifest_type,
        license_url: Some(license_url),
        license_headers: HashMap::new(),
    })
}

async fn fetch_media_info(
    http_client: &HttpClient,
    media_id: &str,
    country_code: &str,
) -> Result<Value> {
    let video_url = FRANCETV_VIDEO_INFO_URL_TEMPLATE.replace("{}", media_id);
    let mut url = Url::parse(&video_url)
        .with_context(|| format!("Invalid FranceTV K7 URL for `{}`.", media_id))?;
    url.query_pairs_mut()
        .append_pair("country_code", country_code)
        .append_pair("capabilities", "drm")
        .append_pair("os", "androidtv")
        .append_pair("diffusion_mode", "tunnel_first")
        .append_pair("offline", "false");

    http_client
        .get_json_for_request(http::Method::GET, url.as_str(), &generic_headers(), None)
        .await
        .with_context(|| format!("Failed to fetch FranceTV K7 media info for `{}`.", media_id))
}

async fn fetch_signed_manifest_url(
    http_client: &HttpClient,
    token_url: &str,
    manifest_url: &str,
) -> Result<String> {
    let mut url = Url::parse(token_url.trim())
        .context("Invalid FranceTV token URL returned by the K7 response.")?;
    let has_format = url.query_pairs().any(|(key, _)| key == "format");
    {
        let mut query = url.query_pairs_mut();
        if !has_format {
            query.append_pair("format", "json");
        }
        query.append_pair("url", manifest_url.trim());
    }

    let payload = http_client
        .get_json_for_request(http::Method::GET, url.as_str(), &generic_headers(), None)
        .await
        .context("Failed to sign the FranceTV media manifest URL.")?;

    read_json_string(
        &payload,
        &["url"],
        "Missing signed FranceTV manifest URL in token response.",
    )
}

async fn fetch_widevine_authorization_token(
    http_client: &HttpClient,
    token_url: &str,
    media_id: &str,
) -> Result<String> {
    let body = json!({
        "id": media_id,
        "drm_type": "widevine",
        "license_type": "online"
    })
    .to_string();
    let headers = HashMap::from([
        ("content-type".to_string(), "application/json".to_string()),
        ("origin".to_string(), FRANCETV_BASE_URL.to_string()),
        ("referer".to_string(), format!("{}/", FRANCETV_BASE_URL)),
        ("user-agent".to_string(), USER_AGENT.to_string()),
    ]);

    let payload = http_client
        .get_json_for_request(http::Method::POST, token_url.trim(), &headers, Some(&body))
        .await
        .context("Failed to fetch FranceTV Widevine authorization token.")?;

    read_first_json_string(
        &payload,
        &[&["token"], &["access_token"], &["authorization"]],
        "Missing FranceTV Widevine authorization token in DRM response.",
    )
}

fn token_url_from_video(video: &Value, prefer_drm: bool) -> String {
    let Some(token) = video.get("token") else {
        return FRANCETV_DEFAULT_TOKEN_URL.to_string();
    };

    if let Some(url) = token
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return url.to_string();
    }

    let Some(token_object) = token.as_object() else {
        return FRANCETV_DEFAULT_TOKEN_URL.to_string();
    };

    let preferred_keys = if prefer_drm {
        ["drm", "akamai"]
    } else {
        ["akamai", "drm"]
    };

    for key in preferred_keys {
        if let Some(url) = token_object
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return url.to_string();
        }
    }

    token_object
        .values()
        .find_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| FRANCETV_DEFAULT_TOKEN_URL.to_string())
}

fn manifest_type_from_format_or_url(format: &str, url: &str) -> String {
    let format = format.to_ascii_lowercase();
    let url = url.to_ascii_lowercase();
    if format.contains("hls") || url.contains(".m3u8") {
        return "m3u8".to_string();
    }

    "mpd".to_string()
}

fn parameter_value(parameters: &[ScraperQueryCollectionParameter], name: &str) -> Option<String> {
    parameters
        .iter()
        .find(|parameter| parameter.name.trim().eq_ignore_ascii_case(name))
        .map(|parameter| parameter.value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn generic_headers() -> HashMap<String, String> {
    HashMap::from([("user-agent".to_string(), USER_AGENT.to_string())])
}

fn normalize_francetv_stream_kind(stream_kind: Option<String>) -> String {
    stream_kind
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| FRANCETV_PROXY_STREAM_KIND.to_string())
}

fn read_first_json_string(
    payload: &Value,
    paths: &[&[&str]],
    error_message: &str,
) -> Result<String> {
    for path in paths {
        if let Ok(value) = read_json_string(payload, path, error_message) {
            return Ok(value);
        }
    }

    bail!(error_message.to_string())
}

fn read_json_string(payload: &Value, path: &[&str], error_message: &str) -> Result<String> {
    let mut current = payload;

    for segment in path {
        current = current
            .get(*segment)
            .with_context(|| error_message.to_string())?;
    }

    if let Some(value) = current.as_str() {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    if let Some(value) = current.as_i64() {
        return Ok(value.to_string());
    }

    if let Some(value) = current.as_u64() {
        return Ok(value.to_string());
    }

    bail!(error_message.to_string())
}

fn save_francetv_license_proxy_url(
    authorization_token: &str,
    stream_kind: &str,
    license_url: &str,
) -> String {
    let token = save_cached_license(authorization_token, stream_kind, license_url);
    format!(
        "{}{}/{}",
        STREAM_PROXY_PATH_PREFIX,
        FRANCETV_SERVICE_ID.trim_matches('/'),
        token
    )
}

fn save_cached_license(authorization_token: &str, stream_kind: &str, license_url: &str) -> String {
    let cache = FRANCETV_LICENSE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let token: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();

    let Ok(mut cache_guard) = cache.lock() else {
        return token;
    };

    let now = Instant::now();
    cache_guard.retain(|_, entry| entry.expires_at > now);
    cache_guard.insert(
        token.clone(),
        CachedFrancetvLicense {
            authorization_token: authorization_token.to_string(),
            license_url: license_url.to_string(),
            stream_kind: stream_kind.to_string(),
            expires_at: now + FRANCETV_LICENSE_TTL,
        },
    );

    token
}

fn load_cached_license(token: &str) -> Option<CachedFrancetvLicense> {
    let cache = FRANCETV_LICENSE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache_guard) = cache.lock() else {
        return None;
    };

    let now = Instant::now();
    cache_guard.retain(|_, entry| entry.expires_at > now);
    cache_guard.get(token).cloned()
}

async fn proxy_francetv_license_request(
    token: &str,
    challenge_body: &[u8],
) -> Result<ProxiedStreamResponse> {
    let normalized_token = token.trim();
    if normalized_token.is_empty() {
        bail!("Missing FranceTV stream token identifier.");
    }

    let cached = load_cached_license(normalized_token)
        .context("Expired or missing FranceTV stream token.")?;
    if cached.stream_kind != FRANCETV_PROXY_STREAM_KIND {
        bail!("Unsupported FranceTV stream kind `{}`.", cached.stream_kind);
    }

    let response = rquest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(25))
        .build()
        .context("Failed to build the FranceTV license proxy client.")?
        .post(cached.license_url.trim())
        .header("nv-authorizations", cached.authorization_token.trim())
        .header("origin", FRANCETV_BASE_URL)
        .header("referer", format!("{}/", FRANCETV_BASE_URL))
        .header(
            CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        )
        .body(challenge_body.to_vec())
        .send()
        .await
        .context("Failed to call the FranceTV Widevine license server.")?;

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
        .context("Failed to read the FranceTV license response.")?
        .to_vec();

    if !status.is_success() {
        let details = String::from_utf8(body)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("HTTP {}", status));
        bail!("FranceTV license request failed: {}", details);
    }

    Ok(ProxiedStreamResponse {
        body,
        content_type,
        headers: HashMap::from([("cache-control".to_string(), "no-store".to_string())]),
    })
}
