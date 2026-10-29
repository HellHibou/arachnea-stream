use anyhow::{bail, Context, Result};
use arachnea_proxy::http::{
    actions::{ProxyHttpActionConfig, ReplaceAll},
    proxy_service::proxied_url_with_countries_and_policy,
};
use async_trait::async_trait;
use rand::{distr::Alphanumeric, Rng};
use rquest::Url;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use arachnea_core::persistence::CredentialsStore;
use arachnea_scrapyfy::{
    HttpClient, ScraperAgregator, ScraperHttpConfig, ScraperQueryCollectionParameter,
};

use crate::services::player_resolver::{
    Chapter, PlayerResolverEndpoints, PlayerStreamRequest, PlayerStreamResolver,
    ProxiedStreamResponse, ResolvedPlayerStream,
};

use crate::PLAYER_DEFAULT_USER_AGENT as USER_AGENT;

/// Stable source identifier for FranceTV routing and credentials.
const FRANCETV_SERVICE_ID: &str = "francetv";

/// Website origin used by the FranceTV resolver.
const FRANCETV_BASE_URL: &str = "https://www.france.tv";

/// FranceTV video metadata URL template.
const FRANCETV_VIDEO_INFO_URL_TEMPLATE: &str = "https://k7.ftven.fr/videos/{}";

/// FranceTV default authorization token endpoint.
const FRANCETV_DEFAULT_TOKEN_URL: &str = "https://hdfauth.ftven.fr/esi/TA";

/// FranceTV Widevine license endpoint.
const FRANCETV_WIDEVINE_LICENSE_URL: &str =
    "https://api-drm.ftven.fr/v1/wvls/contentlicenseservice/v1/licenses/";

/// Stream-kind identifier selecting the FranceTV license proxy.
const FRANCETV_PROXY_STREAM_KIND: &str = "francetv-license-proxy";

/// Default country used by FranceTV when no proxy country override is supplied.
const FRANCETV_PROXY_COUNTRY: &str = "FR";

/// HTTP status classified as a destination rejection for FranceTV.
const FRANCETV_PROXY_REJECTION_STATUS: u16 = 403;

/// Lifetime of cached FranceTV license proxy tokens.
const FRANCETV_LICENSE_TTL: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// Alphanumeric character count of FranceTV proxy affinity identifiers, independent of license tokens.
const FRANCETV_PROXY_AFFINITY_LENGTH: usize = 32;

/// Process-wide cache of FranceTV license proxy requests.
static FRANCETV_LICENSE_CACHE: OnceLock<Mutex<HashMap<String, CachedFrancetvLicense>>> =
    OnceLock::new();

/// Cached FranceTV authorization data with its routing settings and expiry.
#[derive(Clone)]
struct CachedFrancetvLicense {
    authorization_token: String,
    license_url: String,
    stream_kind: String,
    proxy_countries: Vec<String>,
    proxy_affinity: String,
    expires_at: Instant,
}

/// FranceTV implementation of the generic protected playback resolver contract.
pub(crate) struct FrancetvResolver;

#[async_trait]
impl PlayerStreamResolver for FrancetvResolver {
    fn source_id(&self) -> &'static str {
        FRANCETV_SERVICE_ID
    }

    fn resolver_ids(&self) -> &'static [&'static str] {
        &["francetv-video", "francetv-live"]
    }

    async fn get_stream(
        &self,
        scraper_agregator: &ScraperAgregator,
        _credentials_store: &dyn CredentialsStore,
        request: PlayerStreamRequest<'_>,
        endpoints: &PlayerResolverEndpoints,
    ) -> Result<ResolvedPlayerStream> {
        let PlayerStreamRequest {
            resolver,
            target,
            service_parameters,
            proxy_countries,
        } = request;
        match resolver.trim() {
            "francetv-video" => {
                resolve_francetv_stream(
                    scraper_agregator,
                    target,
                    None,
                    service_parameters,
                    proxy_countries,
                    endpoints,
                    false,
                )
                .await
            }
            "francetv-live" => {
                resolve_francetv_stream(
                    scraper_agregator,
                    target,
                    None,
                    service_parameters,
                    proxy_countries,
                    endpoints,
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

    async fn get_drm_license(
        &self,
        scraper_agregator: &ScraperAgregator,
        stream_token: &str,
        body: &[u8],
    ) -> Result<ProxiedStreamResponse> {
        proxy_francetv_license_request(scraper_agregator, stream_token, body).await
    }
}

/// Resolves a FranceTV video or live stream into a protected player stream.
///
/// The K7 metadata determines the manifest URL, DRM mode, chapters and token
/// endpoints. Signed manifest URLs and license requests use a stable per-call
/// proxy affinity.
///
/// # Arguments
/// * `scraper_agregator` - Aggregator used to create the HTTP client.
/// * `media_id` - FranceTV media identifier; surrounding whitespace is ignored.
/// * `stream_kind` - Optional caller-selected stream kind.
/// * `service_parameters` - Service parameters, including optional `country_code`.
/// * `proxy_countries` - Countries selected for outbound routing.
/// * `endpoints` - Public endpoint settings used to build proxy URLs.
/// * `is_live` - Whether to resolve a live stream rather than replay.
///
/// # Errors
/// Returns an error when the identifier is empty, metadata or token retrieval
/// fails, or required response fields are missing.
///
/// # Returns
/// The resolved stream with proxied manifest, chapters and optional DRM proxy.
async fn resolve_francetv_stream(
    scraper_agregator: &ScraperAgregator,
    media_id: &str,
    stream_kind: Option<String>,
    service_parameters: &[ScraperQueryCollectionParameter],
    proxy_countries: &[String],
    endpoints: &PlayerResolverEndpoints,
    is_live: bool,
) -> Result<ResolvedPlayerStream> {
    let normalized_media_id = media_id.trim();
    if normalized_media_id.is_empty() {
        bail!("Missing FranceTV media identifier.");
    }

    let country_code =
        parameter_value(service_parameters, "country_code").unwrap_or_else(|| "FR".to_string());
    let proxy_countries = francetv_proxy_countries(proxy_countries);
    let proxy_affinity = random_proxy_affinity();
    let http_client = scraper_agregator
        .create_http_client(francetv_http_config(&proxy_countries, &proxy_affinity));
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
    let chapters = extract_chapters(video);

    let manifest_token_url = if drm_enabled && !is_live {
        FRANCETV_DEFAULT_TOKEN_URL.to_string()
    } else {
        token_url_from_video(video, false)
    };
    let stream_url =
        fetch_signed_manifest_url(&http_client, &manifest_token_url, &raw_manifest_url).await?;
    let manifest_type = manifest_type_from_format_or_url(format, &stream_url);
    let stream_actions = stream_headers();
    let stream_url_proxy = proxied_url_with_countries_and_policy(
        &stream_url,
        endpoints.http_proxy_public_path.as_deref(),
        &proxy_countries,
        Some(&proxy_affinity),
        &[FRANCETV_PROXY_REJECTION_STATUS],
        &stream_actions,
        &[],
    );

    if !drm_enabled {
        return Ok(ResolvedPlayerStream {
            stream_url: vec![stream_url_proxy],
            manifest_type: Some(manifest_type),
            license_url: None,
            license_headers: HashMap::new(),
            chapters,
            ..Default::default()
        });
    }

    let drm_token_url = token_url_from_video(video, true);
    let authorization_token =
        fetch_widevine_authorization_token(&http_client, &drm_token_url, normalized_media_id)
            .await?;
    let stream_kind = normalize_francetv_stream_kind(stream_kind);
    let license_url = save_francetv_license_proxy_url(
        endpoints,
        &authorization_token,
        &stream_kind,
        FRANCETV_WIDEVINE_LICENSE_URL,
        &proxy_countries,
        &proxy_affinity,
    );

    Ok(ResolvedPlayerStream {
        stream_url: vec![stream_url_proxy],
        manifest_type: Some(manifest_type),
        license_url: Some(license_url),
        license_headers: HashMap::new(),
        chapters,
        ..Default::default()
    })
}

/// Extracts and orders FranceTV marker chapters from a video payload.
///
/// # Arguments
/// * `video` - FranceTV K7 `video` object.
///
/// # Returns
/// Chapters sorted by start time, or `None` when no valid marker exists.
fn extract_chapters(video: &Value) -> Option<Vec<Chapter>> {
    let video_duration = video
        .get("duration")
        .and_then(Value::as_f64)
        .filter(|duration| duration.is_finite() && *duration > 0.0);
    let coming_next = extract_chapter(video, "coming_next", "coming_next", video_duration);
    let mut outro = extract_chapter(video, "closing_credits", "outro", video_duration);

    if let (Some(coming_next), Some(current_outro)) = (&coming_next, &mut outro) {
        if (coming_next.start - current_outro.start).abs() <= 0.001 {
            if coming_next.end < current_outro.end {
                current_outro.start = coming_next.end;
            } else {
                outro = None;
            }
        }
    }

    let mut chapters = [
        extract_chapter(video, "previously", "previously", video_duration),
        coming_next,
        extract_chapter(video, "skip_intro", "intro", video_duration),
        outro,
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

    chapters.sort_by(|left, right| left.start.total_cmp(&right.start));
    (!chapters.is_empty()).then_some(chapters)
}

/// Extracts one chapter from a FranceTV marker.
///
/// # Arguments
/// * `video` - FranceTV K7 `video` object.
/// * `marker_name` - Marker field containing `timecode` and `duration`.
/// * `chapter_type` - Chapter type assigned to the result.
/// * `video_duration` - Optional media duration used as a fallback end bound.
///
/// # Returns
/// The chapter, or `None` when the marker is absent or its timing is invalid.
fn extract_chapter(
    video: &Value,
    marker_name: &str,
    chapter_type: &str,
    video_duration: Option<f64>,
) -> Option<Chapter> {
    let marker = video.get(marker_name)?;
    let start = marker.get("timecode").and_then(Value::as_f64)?;
    let end = marker
        .get("duration")
        .and_then(Value::as_f64)
        .map(|duration| start + duration)
        .or(video_duration)
        .map(|end| video_duration.map_or(end, |duration| end.min(duration)))?;

    if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start {
        return None;
    }

    Some(Chapter {
        start,
        end,
        title: None,
        chapter_type: chapter_type.to_string(),
    })
}

/// Fetches FranceTV K7 media metadata for a media identifier.
///
/// # Arguments
/// * `http_client` - HTTP client used for the request.
/// * `media_id` - FranceTV media identifier.
/// * `country_code` - Country sent to K7 for availability checks.
///
/// # Errors
/// Returns an error when the URL is invalid or the request fails.
///
/// # Returns
/// The decoded K7 payload.
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

/// Exchanges a manifest URL for a signed FranceTV playback URL.
///
/// # Arguments
/// * `http_client` - HTTP client used for the token request.
/// * `token_url` - FranceTV signing endpoint from the K7 metadata.
/// * `manifest_url` - Unsigned manifest URL.
///
/// # Errors
/// Returns an error when the token URL is invalid, the request fails, or the
/// response has no signed URL.
///
/// # Returns
/// The signed manifest URL.
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

/// Fetches a Widevine authorization token for FranceTV DRM.
///
/// # Arguments
/// * `http_client` - HTTP client used for the token request.
/// * `token_url` - FranceTV DRM authorization endpoint.
/// * `media_id` - FranceTV media identifier.
///
/// # Errors
/// Returns an error when the request fails or the response has no usable token.
///
/// # Returns
/// The authorization token accepted by the license endpoint.
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

/// Selects the token endpoint exposed by a FranceTV video payload.
///
/// # Arguments
/// * `video` - FranceTV K7 `video` object.
/// * `prefer_drm` - Whether DRM token endpoints take precedence over Akamai.
///
/// # Returns
/// The selected token URL, falling back to `FRANCETV_DEFAULT_TOKEN_URL` when
/// no usable URL is present.
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

/// Determines the manifest type from the K7 format or signed URL.
///
/// # Arguments
/// * `format` - Source-declared format string.
/// * `url` - Signed manifest URL.
///
/// # Returns
/// `m3u8` when either value identifies HLS, otherwise `mpd`.
fn manifest_type_from_format_or_url(format: &str, url: &str) -> String {
    let format = format.to_ascii_lowercase();
    let url = url.to_ascii_lowercase();
    if format.contains("hls") || url.contains(".m3u8") {
        return "m3u8".to_string();
    }

    "mpd".to_string()
}

/// Returns a non-empty service parameter value by case-insensitive name.
///
/// # Arguments
/// * `parameters` - Service parameters to search.
/// * `name` - Case-insensitive parameter name.
///
/// # Returns
/// The trimmed non-empty value, or `None` when absent.
fn parameter_value(parameters: &[ScraperQueryCollectionParameter], name: &str) -> Option<String> {
    parameters
        .iter()
        .find(|parameter| parameter.name.trim().eq_ignore_ascii_case(name))
        .map(|parameter| parameter.value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Builds the generic FranceTV request headers.
///
/// # Returns
/// Headers containing the resolver user-agent.
fn generic_headers() -> HashMap<String, String> {
    HashMap::from([("user-agent".to_string(), USER_AGENT.to_string())])
}

/// Builds the HTTP configuration used for FranceTV requests.
///
/// # Arguments
/// * `proxy_countries` - Countries to use for outbound routing.
/// * `proxy_affinity` - Stable affinity shared by related requests.
///
/// # Returns
/// A configuration treating HTTP 403 as a destination rejection.
fn francetv_http_config(proxy_countries: &[String], proxy_affinity: &str) -> ScraperHttpConfig {
    ScraperHttpConfig {
        proxy_rejection_statuses: vec![FRANCETV_PROXY_REJECTION_STATUS],
        ..ScraperHttpConfig::default()
            .proxy_countries(proxy_countries.to_vec())
            .proxy_affinity(proxy_affinity)
    }
}

/// Returns the proxy countries to use, defaulting to France.
///
/// # Arguments
/// * `proxy_countries` - Caller-selected countries.
///
/// # Returns
/// A copy of the selected countries, or `[FRANCETV_PROXY_COUNTRY]` when empty.
fn francetv_proxy_countries(proxy_countries: &[String]) -> Vec<String> {
    if proxy_countries.is_empty() {
        vec![FRANCETV_PROXY_COUNTRY.to_string()]
    } else {
        proxy_countries.to_vec()
    }
}

/// Generates a random proxy affinity identifier.
///
/// # Returns
/// A string of `FRANCETV_PROXY_AFFINITY_LENGTH` random characters.
fn random_proxy_affinity() -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(FRANCETV_PROXY_AFFINITY_LENGTH)
        .map(char::from)
        .collect()
}

/// Normalizes a FranceTV stream kind to the supported license proxy kind.
///
/// # Arguments
/// * `stream_kind` - Optional caller-selected stream kind.
///
/// # Returns
/// The trimmed non-empty kind, or `FRANCETV_PROXY_STREAM_KIND` when absent or
/// blank.
fn normalize_francetv_stream_kind(stream_kind: Option<String>) -> String {
    stream_kind
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| FRANCETV_PROXY_STREAM_KIND.to_string())
}

/// Returns the first readable non-empty scalar from several JSON paths.
///
/// # Arguments
/// * `payload` - JSON value to traverse.
/// * `paths` - Candidate key paths tried in order.
/// * `error_message` - Message used when every path fails.
///
/// # Errors
/// Returns an error when no path yields a supported non-empty scalar.
///
/// # Returns
/// The first trimmed scalar value.
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

/// Reads a non-empty string or integer from a nested JSON path.
///
/// # Arguments
/// * `payload` - JSON value to traverse.
/// * `path` - Object keys applied successively.
/// * `error_message` - Message used when a key or scalar is missing.
///
/// # Errors
/// Returns an error when traversal fails or the target is neither a non-empty
/// string nor an integer.
///
/// # Returns
/// The trimmed string representation of the scalar.
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

/// Caches FranceTV license data and returns its public proxy URL.
///
/// # Arguments
/// * `endpoints` - Public endpoint settings used to build the proxy URL.
/// * `authorization_token` - FranceTV Widevine authorization token.
/// * `stream_kind` - Stream kind validated when the token is used.
/// * `license_url` - Upstream Widevine license URL.
/// * `proxy_countries` - Countries to reuse for the license request.
/// * `proxy_affinity` - Affinity to reuse for the license request.
///
/// # Returns
/// The public license proxy URL containing a generated token.
fn save_francetv_license_proxy_url(
    endpoints: &PlayerResolverEndpoints,
    authorization_token: &str,
    stream_kind: &str,
    license_url: &str,
    proxy_countries: &[String],
    proxy_affinity: &str,
) -> String {
    let token = save_cached_license(
        authorization_token,
        stream_kind,
        license_url,
        proxy_countries,
        proxy_affinity,
    );
    endpoints.drm_license_url(FRANCETV_SERVICE_ID, &token)
}

/// Stores a FranceTV license request under a newly generated token.
///
/// # Arguments
/// * `authorization_token` - FranceTV Widevine authorization token.
/// * `stream_kind` - Stream kind validated when the token is used.
/// * `license_url` - Upstream Widevine license URL.
/// * `proxy_countries` - Countries to reuse for the license request.
/// * `proxy_affinity` - Affinity to reuse for the license request.
///
/// # Returns
/// The generated token. The entry is not stored when the cache mutex is
/// poisoned.
fn save_cached_license(
    authorization_token: &str,
    stream_kind: &str,
    license_url: &str,
    proxy_countries: &[String],
    proxy_affinity: &str,
) -> String {
    let cache = FRANCETV_LICENSE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let token: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(crate::PLAYER_PROXY_TOKEN_LENGTH)
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
            proxy_countries: proxy_countries.to_vec(),
            proxy_affinity: proxy_affinity.to_string(),
            expires_at: now + FRANCETV_LICENSE_TTL,
        },
    );

    token
}

/// Loads a non-expired FranceTV license request, pruning expired entries.
///
/// # Arguments
/// * `token` - Public token used as the cache key.
///
/// # Returns
/// The cached license request, or `None` when absent, expired, or the cache
/// mutex is poisoned.
fn load_cached_license(token: &str) -> Option<CachedFrancetvLicense> {
    let cache = FRANCETV_LICENSE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache_guard) = cache.lock() else {
        return None;
    };

    let now = Instant::now();
    cache_guard.retain(|_, entry| entry.expires_at > now);
    cache_guard.get(token).cloned()
}

/// Proxies a Widevine challenge using a cached FranceTV license request.
///
/// # Arguments
/// * `scraper_agregator` - Aggregator used to create the HTTP client.
/// * `token` - Public token identifying the cached license request.
/// * `challenge_body` - Raw Widevine challenge bytes.
///
/// # Errors
/// Returns an error when the token is empty, expired or unsupported, or when
/// the upstream request fails or is rejected.
///
/// # Returns
/// The upstream license response with a `no-store` cache policy.
async fn proxy_francetv_license_request(
    scraper_agregator: &ScraperAgregator,
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

    let response = scraper_agregator
        .create_http_client(francetv_http_config(
            &cached.proxy_countries,
            &cached.proxy_affinity,
        ))
        .send_bytes_for_request(
            http::Method::POST,
            cached.license_url.trim(),
            &HashMap::from([
                ("nv-authorizations".to_string(), cached.authorization_token),
                ("origin".to_string(), FRANCETV_BASE_URL.to_string()),
                ("referer".to_string(), format!("{}/", FRANCETV_BASE_URL)),
                (
                    "content-type".to_string(),
                    "application/octet-stream".to_string(),
                ),
            ]),
            Some(challenge_body.to_vec()),
        )
        .await
        .context("Failed to call the FranceTV Widevine license server.")?;

    let status = response.status();
    let content_type = response
        .headers()
        .get(http::header::CONTENT_TYPE)
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

/// Builds proxy actions that rewrite FranceTV HLS key URLs.
///
/// # Returns
/// Replacement actions scoped to HLS manifest content types.
fn stream_headers() -> Vec<ProxyHttpActionConfig> {
    let hls_content_types = hls_manifest_content_types();

    vec![ReplaceAll::config(
        r#"#EXT-X-(SESSION-)?KEY:([^\r\n]*?)URI="(https://[^/"]+/keys/[^"]+\.key)""#,
        r#"#EXT-X-${1}KEY:${2}URI="{proxy_inherited}/${3}""#,
        Some(hls_content_types.clone()),
    )]
}

/// Returns the content types identifying HLS manifests.
///
/// # Returns
/// The supported HLS MIME types.
fn hls_manifest_content_types() -> Vec<String> {
    [
        "application/vnd.apple.mpegurl",
        "application/x-mpegurl",
        "audio/mpegurl",
        "audio/x-mpegurl",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}
