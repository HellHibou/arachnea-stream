use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use rand::{distr::Alphanumeric, Rng};
use rquest::header::{HeaderValue, CONTENT_TYPE};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use arachnea_core::persistence::CredentialsStore;
use arachnea_scrapyfy::{ScraperAgregator, ScraperQueryCollectionParameter};

const DEFAULT_DRM_LICENSE_PUBLIC_PATH: &str = "/api/get_drm_license";
const DEFAULT_STREAM_KIND: &str = "widevine-license-proxy";
const DRM_TODAY_TOKEN_TTL: Duration = Duration::from_secs(15 * 60);

static DRM_TODAY_TOKEN_CACHE: OnceLock<Mutex<HashMap<String, CachedDrmTodayLicenseToken>>> =
    OnceLock::new();

/// A single chapter entry within a resolved player stream.
#[derive(Serialize)]
pub(crate) struct Chapter {
    /// Start time of the chapter in seconds.
    pub start: f64,
    /// End time of the chapter in seconds.
    pub end: f64,
    /// Display title of the chapter.
    pub title: String,
    /// Type discriminator for the chapter (e.g. "chapter").
    #[serde(rename = "type")]
    pub chapter_type: String,
}

/// Sprite thumbnail metadata for video storyboards.
#[derive(Serialize)]
pub(crate) struct SpriteThumbnail {
    /// The URL to the sprite thumbnail image.
    pub url: String,
    /// The width of each thumbnail in the sprite.
    pub width: u32,
    /// The height of each thumbnail in the sprite.
    pub height: u32,
    /// The number of columns in the sprite image.
    pub columns: u32,
    /// The number of thumbnail rows in each sprite image.
    pub rows: u32,
    /// Optional index used for the first sprite image in a sequential URL template.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_index: Option<u32>,
    /// The time interval between thumbnails in seconds.
    pub interval: f64,
}

/// Link metadata for one resolved player image.
#[derive(Serialize)]
pub(crate) struct ResolvedPlayerImageTitle {
    /// URL of the image resource.
    pub link: String,
}

/// Playback stream resolved by a source-specific player resolver.
#[derive(Default, Serialize)]
pub(crate) struct ResolvedPlayerStream {
    /// Display title extracted from the player page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional title image metadata extracted from the player page.
    #[serde(rename = "image/title", skip_serializing_if = "Option::is_none")]
    pub image_title: Option<ResolvedPlayerImageTitle>,
    /// Ordered list of alternative stream manifest URLs (primary first).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stream_url: Vec<String>,
    /// Manifest type consumed by the frontend player, such as `mpd` or `m3u8`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_type: Option<String>,
    /// Headers embedded in generated proxy stream URLs.
    #[serde(skip_serializing)]
    pub stream_headers: HashMap<String, String>,
    /// Same-origin license proxy URL for protected streams.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license_url: Option<String>,
    /// Extra headers sent by the frontend player when requesting the license.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub license_headers: HashMap<String, String>,
    /// Optional WebVTT sprite metadata URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vtt_url: Option<String>,
    /// Optional sprite storyboard metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storyboard: Option<SpriteThumbnail>,
    /// Optional ordered list of chapters extracted from the player metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chapters: Option<Vec<Chapter>>,
}

/// Binary response returned by a resolver-owned stream proxy.
pub(crate) struct ProxiedStreamResponse {
    /// Response body forwarded to the media player.
    pub body: Vec<u8>,
    /// MIME type returned to the caller.
    pub content_type: String,
    /// Additional response headers.
    pub headers: HashMap<String, String>,
}

/// Browser-facing stream endpoints available to source-specific resolvers.
#[derive(Clone)]
pub(crate) struct PlayerResolverEndpoints {
    /// Public path for the generic HTTP proxy stream command.
    pub http_proxy_public_path: Option<String>,
    /// Public path for DRM license proxy requests.
    pub drm_license_public_path: String,
}

impl Default for PlayerResolverEndpoints {
    fn default() -> Self {
        Self {
            http_proxy_public_path: None,
            drm_license_public_path: DEFAULT_DRM_LICENSE_PUBLIC_PATH.to_string(),
        }
    }
}

impl PlayerResolverEndpoints {
    /// Builds the public URL for one source-owned DRM license token.
    pub fn drm_license_url(&self, service_id: &str, token_id: &str) -> String {
        format!(
            "{}/{}/{}",
            self.drm_license_public_path.trim_end_matches('/'),
            service_id.trim_matches('/'),
            token_id.trim_matches('/')
        )
    }
}

/// Common contract implemented by source-specific protected playback resolvers.
#[async_trait]
pub(crate) trait PlayerStreamResolver: Send + Sync {
    /// Stable service/source identifier used by YAML and stream routing.
    fn source_id(&self) -> &'static str;

    /// Globally unique resolver identifiers handled by this implementation.
    fn resolver_ids(&self) -> &'static [&'static str];

    /// Resolves one YAML player descriptor into a playable stream.
    async fn get_stream(
        &self,
        scraper_agregator: &ScraperAgregator,
        credentials_store: &dyn CredentialsStore,
        resolver: &str,
        target: &str,
        service_parameters: &[ScraperQueryCollectionParameter],
        endpoints: &PlayerResolverEndpoints,
    ) -> Result<ResolvedPlayerStream>;

    /// Handles a follow-up binary stream request owned by this resolver.
    async fn get_drm_license(
        &self,
        scraper_agregator: &ScraperAgregator,
        stream_token: &str,
        body: &[u8],
    ) -> Result<ProxiedStreamResponse>;
}

#[derive(Clone)]
struct CachedDrmTodayLicenseToken {
    token: DrmTodayLicenseToken,
    expires_at: Instant,
}

#[derive(Clone)]
struct DrmTodayLicenseToken {
    auth_token: String,
    stream_kind: String,
    license_url: String,
    customer_name: Option<String>,
}

/// Normalizes a resolver stream kind to the default supported DRM proxy kind.
pub(crate) fn normalize_stream_kind(stream_kind: Option<String>) -> String {
    stream_kind
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_STREAM_KIND.to_string())
}

/// Stores a DRM Today token and returns the same-origin proxy URL exposed to the frontend.
pub(crate) fn save_drm_today_license_proxy_url(
    endpoints: &PlayerResolverEndpoints,
    service_id: &str,
    auth_token: &str,
    stream_kind: &str,
    license_url: &str,
    customer_name: Option<&str>,
) -> String {
    let token_id = save_drm_today_license_token(
        auth_token,
        stream_kind,
        license_url,
        customer_name.map(str::to_string),
    );

    endpoints.drm_license_url(service_id, &token_id)
}

/// Proxies one Widevine challenge to DRM Today using the cached source token.
pub(crate) async fn proxy_drm_today_license_request(
    token_id: &str,
    challenge_body: &[u8],
) -> Result<ProxiedStreamResponse> {
    let normalized_token_id = token_id.trim();
    if normalized_token_id.is_empty() {
        bail!("Missing DRM Today license token identifier.");
    }

    let cached_token = load_cached_drm_today_license_token(normalized_token_id)
        .context("Expired or missing DRM Today stream token.")?;

    match cached_token.stream_kind.as_str() {
        DEFAULT_STREAM_KIND => proxy_license_request(&cached_token, challenge_body).await,
        stream_kind => bail!("Unsupported DRM Today stream kind `{}`.", stream_kind),
    }
}

fn save_drm_today_license_token(
    auth_token: &str,
    stream_kind: &str,
    license_url: &str,
    customer_name: Option<String>,
) -> String {
    let cache = DRM_TODAY_TOKEN_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let token_id: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();

    let Ok(mut cache_guard) = cache.lock() else {
        return token_id;
    };

    let now = Instant::now();
    cache_guard.retain(|_, entry| entry.expires_at > now);
    cache_guard.insert(
        token_id.clone(),
        CachedDrmTodayLicenseToken {
            token: DrmTodayLicenseToken {
                auth_token: auth_token.to_string(),
                stream_kind: stream_kind.to_string(),
                license_url: license_url.to_string(),
                customer_name,
            },
            expires_at: now + DRM_TODAY_TOKEN_TTL,
        },
    );

    token_id
}

fn load_cached_drm_today_license_token(token_id: &str) -> Option<DrmTodayLicenseToken> {
    let cache = DRM_TODAY_TOKEN_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache_guard) = cache.lock() else {
        return None;
    };

    let now = Instant::now();
    cache_guard.retain(|_, entry| entry.expires_at > now);
    cache_guard.get(token_id).map(|entry| entry.token.clone())
}

async fn proxy_license_request(
    token: &DrmTodayLicenseToken,
    challenge_body: &[u8],
) -> Result<ProxiedStreamResponse> {
    let normalized_upfront_token = token.auth_token.trim();
    if normalized_upfront_token.is_empty() {
        bail!("Missing DRM Today auth token.");
    }

    let normalized_license_url = token.license_url.trim();
    if normalized_license_url.is_empty() {
        bail!("Missing DRM Today license URL.");
    }

    let mut request = rquest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:148.0) Gecko/20100101 Firefox/148.0",
        )
        .timeout(Duration::from_secs(25))
        .build()
        .context("Failed to build the DRM Today license proxy client.")?
        .post(normalized_license_url)
        .header("x-dt-auth-token", normalized_upfront_token)
        .header(
            CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        );

    if let Some(customer_name) = token
        .customer_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        request = request.header("x-customer-name", customer_name);
    }

    let response = request
        .body(challenge_body.to_vec())
        .send()
        .await
        .context("Failed to call the DRM Today Widevine license server.")?;

    let status = response.status();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string())
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let body = response
        .bytes()
        .await
        .context("Failed to read the DRM Today Widevine license response.")?
        .to_vec();

    if !status.is_success() {
        let details = String::from_utf8(body)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("HTTP {}", status));
        bail!("DRM Today Widevine license request failed: {}", details);
    }

    let body = normalize_license_response(body, &content_type)?;

    Ok(ProxiedStreamResponse {
        body,
        content_type: "application/octet-stream".to_string(),
        headers: HashMap::from([("cache-control".to_string(), "no-store".to_string())]),
    })
}

fn normalize_license_response(body: Vec<u8>, content_type: &str) -> Result<Vec<u8>> {
    if let Some(decoded_license) = try_extract_wrapped_license(&body, content_type)? {
        return Ok(decoded_license);
    }

    Ok(body)
}

fn try_extract_wrapped_license(body: &[u8], content_type: &str) -> Result<Option<Vec<u8>>> {
    let body_as_text = match std::str::from_utf8(body) {
        Ok(value) => value.trim(),
        Err(_) => return Ok(None),
    };

    if body_as_text.is_empty() {
        return Ok(None);
    }

    let looks_like_json = content_type.contains("json")
        || body_as_text.starts_with('{')
        || body_as_text.starts_with('[');
    if !looks_like_json {
        return Ok(None);
    }

    let payload: Value = serde_json::from_str(body_as_text)
        .context("Invalid JSON returned by the DRM Today Widevine license server.")?;

    let Some(encoded_license) = payload
        .get("license")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        bail!(
            "DRM Today Widevine server returned JSON without a usable `license` field: {}",
            body_as_text
        );
    };

    let decoded_license = BASE64_STANDARD
        .decode(encoded_license)
        .context("Invalid base64 license returned by the DRM Today Widevine server.")?;

    Ok(Some(decoded_license))
}
