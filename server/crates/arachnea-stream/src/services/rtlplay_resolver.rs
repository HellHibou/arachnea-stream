use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};
use regex::Regex;
use rquest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Url,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex as AsyncMutex;

use arachnea_core::persistence::CredentialsStore;
use arachnea_scrapyfy::{
    HttpClient, ScraperAgregator, ScraperHttpConfig, ScraperQueryCollectionParameter,
};

use crate::services::player_resolver::{
    normalize_stream_kind, proxy_drm_today_license_request, save_drm_today_license_proxy_url,
    Chapter, PlayerResolverEndpoints, PlayerStreamRequest, PlayerStreamResolver,
    ProxiedStreamResponse, ResolvedPlayerStream,
};

/// Stable source identifier for RTLplay routing and credentials.
const RTLPLAY_SERVICE_ID: &str = "rtlplay-be";

/// Website origin used by the RTLplay resolver.
const BASE_URL: &str = "https://www.rtlplay.be/rtlplay";

/// RTLplay SSO website endpoint.
const SSO_BASE_URL: &str = "https://sso.rtl.be/";

/// RTLplay player configuration URL template.
const URL_CONFIG_TEMPLATE: &str = "https://videoplayer-service.dpgmedia.net/play-config/{}";

/// RTLplay SSO login endpoint.
const URL_SSO_LOGIN: &str = "https://sso.rtl.be/api/account/login";

/// RTLplay SSO authorization endpoint.
const URL_SSO_AUTH: &str = "https://sso.rtl.be/oidc/account/authenticate";

/// RTLplay fallback Widevine license endpoint.
const DEFAULT_LICENSE_URL: &str = "https://lic.drmtoday.com/license-proxy-widevine/cenc/";

/// Public client API identifier expected by RTLplay.
const API_KEY: &str = "2W7kCXUTyUgKf7HKlK9qcYJvFmiPFaBEFT90eC2b";

/// Popcorn SDK version identifier expected by RTLplay.
const POPCORN_SDK: &str = "8";

/// Customer identifier sent to the RTLplay DRM service.
const RTLPLAY_CUSTOMER_NAME: &str = "rtlbe";

/// Fallback lifetime of a cached RTLplay session without a usable JWT expiry.
const RTLPLAY_SESSION_FALLBACK_TTL: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// Maximum number of redirects followed during RTLplay authentication.
const RTLPLAY_AUTH_MAX_REDIRECTS: usize = 12;

/// Gigya cookie name used by the RTLplay authentication flow.
const GIGYA_COOKIE_NAME: &str =
    "gig_bootstrap_3_LGnnaXIFQ_VRXofTaFTGnc6q7pM923yFB0AXSWdxADsUT0y2dVdDKmPRyQMj7LMc";

/// Gigya cookie marker expected by the RTLplay authentication flow.
const GIGYA_COOKIE_VALUE: &str = "_gigya_ver4";

/// Seconds subtracted from the RTLplay JWT expiry before cache reuse.
const RTLPLAY_JWT_REFRESH_MARGIN_SECONDS: u64 = 60;

/// Process-wide cache of authenticated RTLplay sessions keyed by login.
static RTLPLAY_SESSION_CACHE: OnceLock<Mutex<HashMap<String, CachedRtlPlaySession>>> =
    OnceLock::new();
/// Serializes cache misses during the RTLplay login flow.
static RTLPLAY_LOGIN_LOCK: OnceLock<AsyncMutex<()>> = OnceLock::new();
/// Shared regex used to locate `__NEXT_DATA__` payloads.
static NEXT_DATA_REGEX: OnceLock<Regex> = OnceLock::new();

/// Authenticated RTLplay cookie set.
#[derive(Clone)]
struct RtlPlaySession {
    cookies: HashMap<String, String>,
}

/// Cached RTLplay session with its expiry instant.
struct CachedRtlPlaySession {
    session: RtlPlaySession,
    expires_at: Instant,
}

/// Playback data extracted from an RTLplay player configuration.
struct ResolvedRtlPlayVideo {
    manifest_url: String,
    license_url: Option<String>,
    license_token: Option<String>,
    storyboard_vtt_url: Option<String>,
    chapters: Vec<Chapter>,
}

/// RTLplay client version taken from service parameters.
struct RtlPlayApiVersion {
    major: String,
    minor: String,
    build: String,
}

impl RtlPlayApiVersion {
    /// Builds the API version from required service parameters.
    ///
    /// # Arguments
    /// * `parameters` - Service parameters containing `api_version_major`,
    ///   `api_version_minor` and `api_version_build`.
    ///
    /// # Errors
    /// Returns an error when any required parameter is missing or empty.
    ///
    /// # Returns
    /// The validated API version.
    fn from_parameters(parameters: &[ScraperQueryCollectionParameter]) -> Result<Self> {
        Ok(Self {
            major: required_collection_parameter(parameters, "api_version_major")?,
            minor: required_collection_parameter(parameters, "api_version_minor")?,
            build: required_collection_parameter(parameters, "api_version_build")?,
        })
    }
}

/// Returns a required non-empty service parameter value.
///
/// # Arguments
/// * `parameters` - Service parameters to search.
/// * `name` - Exact trimmed parameter name.
///
/// # Errors
/// Returns an error when the parameter is absent or empty.
///
/// # Returns
/// The trimmed parameter value.
fn required_collection_parameter(
    parameters: &[ScraperQueryCollectionParameter],
    name: &str,
) -> Result<String> {
    parameters
        .iter()
        .find(|parameter| parameter.name.trim() == name)
        .map(|parameter| parameter.value.trim().to_string())
        .filter(|value| !value.is_empty())
        .with_context(|| format!("Missing RTL Play service parameter `{}`.", name))
}

/// RTL Play implementation of the generic protected playback resolver contract.
pub(crate) struct RtlPlayResolver;

#[async_trait]
impl PlayerStreamResolver for RtlPlayResolver {
    fn source_id(&self) -> &'static str {
        RTLPLAY_SERVICE_ID
    }

    fn resolver_ids(&self) -> &'static [&'static str] {
        &["rtlplay-video", "rtlplay-live"]
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
            service_parameters,
            proxy_countries: _proxy_countries,
        } = request;
        match resolver.trim() {
            "rtlplay-video" => {
                let api_version = RtlPlayApiVersion::from_parameters(service_parameters)?;
                resolve_replay_stream(
                    scraper_agregator,
                    credentials_store,
                    target,
                    None,
                    &api_version,
                    endpoints,
                )
                .await
            }
            "rtlplay-live" => {
                let api_version = RtlPlayApiVersion::from_parameters(service_parameters)?;
                resolve_live_stream(
                    scraper_agregator,
                    credentials_store,
                    target,
                    None,
                    &api_version,
                    endpoints,
                )
                .await
            }
            kind => bail!(
                "Unsupported player resolver `{}` for source `{}`.",
                kind,
                RTLPLAY_SERVICE_ID
            ),
        }
    }

    async fn get_drm_license(
        &self,
        _scraper_agregator: &ScraperAgregator,
        stream_token: &str,
        body: &[u8],
    ) -> Result<ProxiedStreamResponse> {
        proxy_drm_today_license_request(stream_token, body).await
    }
}

/// Resolves an RTLplay replay video into a protected player stream.
///
/// # Arguments
/// * `scraper_agregator` - Aggregator used to create the HTTP client.
/// * `credentials_store` - Store providing the RTLplay credentials.
/// * `video_id` - RTLplay video identifier; surrounding whitespace is ignored.
/// * `stream_kind` - Optional caller-selected stream kind.
/// * `api_version` - Client version sent to RTLplay.
/// * `endpoints` - Public endpoint settings used to build proxy URLs.
///
/// # Errors
/// Returns an error when the identifier is empty, authentication fails, or
/// playback metadata cannot be resolved.
///
/// # Returns
/// The resolved replay stream.
async fn resolve_replay_stream(
    scraper_agregator: &ScraperAgregator,
    credentials_store: &dyn CredentialsStore,
    video_id: &str,
    stream_kind: Option<String>,
    api_version: &RtlPlayApiVersion,
    endpoints: &PlayerResolverEndpoints,
) -> Result<ResolvedPlayerStream> {
    let normalized_video_id = video_id.trim();
    if normalized_video_id.is_empty() {
        bail!("Missing RTL Play video identifier.");
    }

    let http_client = scraper_agregator
        .create_http_client(rtlplay_http_config())
        .with_isolated_cookies();
    let session = get_or_login_session(&http_client, credentials_store).await?;
    let video_url = format!("{}/player/{}", BASE_URL, normalized_video_id);
    let resolved =
        resolve_final_video_url(&http_client, &session, &video_url, false, api_version).await?;

    build_resolved_player_stream(resolved, stream_kind, endpoints)
}

/// Resolves an RTLplay live channel into a protected player stream.
///
/// # Arguments
/// * `scraper_agregator` - Aggregator used to create the HTTP client.
/// * `credentials_store` - Store providing the RTLplay credentials.
/// * `channel_id` - RTLplay live channel identifier or URL.
/// * `stream_kind` - Optional caller-selected stream kind.
/// * `api_version` - Client version sent to RTLplay.
/// * `endpoints` - Public endpoint settings used to build proxy URLs.
///
/// # Errors
/// Returns an error when the channel is unsupported, authentication fails, or
/// playback metadata cannot be resolved.
///
/// # Returns
/// The resolved live stream.
async fn resolve_live_stream(
    scraper_agregator: &ScraperAgregator,
    credentials_store: &dyn CredentialsStore,
    channel_id: &str,
    stream_kind: Option<String>,
    api_version: &RtlPlayApiVersion,
    endpoints: &PlayerResolverEndpoints,
) -> Result<ResolvedPlayerStream> {
    let normalized_channel = normalize_live_channel(channel_id)?;

    let http_client = scraper_agregator
        .create_http_client(rtlplay_http_config())
        .with_isolated_cookies();
    let session = get_or_login_session(&http_client, credentials_store).await?;
    let video_url = format!("{}/direct/{}", BASE_URL, normalized_channel.as_str());
    let resolved =
        resolve_final_video_url(&http_client, &session, &video_url, true, api_version).await?;
    build_resolved_player_stream(resolved, stream_kind, endpoints)
}

/// Converts RTLplay playback data into the generic player stream contract.
///
/// # Arguments
/// * `resolved` - Playback data extracted from the player configuration.
/// * `stream_kind` - Optional caller-selected stream kind.
/// * `endpoints` - Public endpoint settings used to build license proxy URLs.
///
/// # Returns
/// The resolved player stream. License proxying is included only when a DRM
/// token is present.
fn build_resolved_player_stream(
    resolved: ResolvedRtlPlayVideo,
    stream_kind: Option<String>,
    endpoints: &PlayerResolverEndpoints,
) -> Result<ResolvedPlayerStream> {
    let license_url = resolved.license_token.as_deref().map(|license_token| {
        let stream_kind = normalize_stream_kind(stream_kind);
        save_drm_today_license_proxy_url(
            endpoints,
            RTLPLAY_SERVICE_ID,
            license_token,
            &stream_kind,
            resolved
                .license_url
                .as_deref()
                .unwrap_or(DEFAULT_LICENSE_URL),
            Some(RTLPLAY_CUSTOMER_NAME),
        )
    });

    Ok(ResolvedPlayerStream {
        stream_url: vec![resolved.manifest_url],
        manifest_type: Some("mpd".to_string()),
        license_url,
        storyboard_vtt_url: resolved.storyboard_vtt_url,
        license_headers: HashMap::new(),
        chapters: Some(resolved.chapters),
        ..Default::default()
    })
}

/// Builds the HTTP configuration used for RTLplay authentication and playback.
///
/// # Returns
/// A configuration with `RTLPLAY_AUTH_MAX_REDIRECTS` as the redirect limit.
fn rtlplay_http_config() -> ScraperHttpConfig {
    ScraperHttpConfig {
        max_redirects: Some(RTLPLAY_AUTH_MAX_REDIRECTS),
        ..Default::default()
    }
}

/// Builds the common RTLplay request headers for an API version.
///
/// # Arguments
/// * `api_version` - Client version used in the user-agent and app-version
///   headers.
///
/// # Errors
/// Returns an error when a generated header value is invalid.
///
/// # Returns
/// The headers required by RTLplay endpoints.
fn rtlplay_headers(api_version: &RtlPlayApiVersion) -> Result<HeaderMap> {
    let user_agent = rtlplay_user_agent(api_version);
    let mut map = HeaderMap::new();
    map.insert(
        HeaderName::from_static("user-agent"),
        HeaderValue::from_str(&user_agent).context("Invalid RTL Play user-agent header.")?,
    );
    map.insert(
        HeaderName::from_static("accept"),
        HeaderValue::from_static("*/*"),
    );
    map.insert(
        HeaderName::from_static("accept-encoding"),
        HeaderValue::from_static("gzip"),
    );
    map.insert(
        HeaderName::from_static("connection"),
        HeaderValue::from_static("Keep-Alive"),
    );
    map.insert(
        HeaderName::from_static("content-type"),
        HeaderValue::from_static("application/json; charset=UTF-8"),
    );
    map.insert(
        HeaderName::from_static("lfvp-device-segment"),
        HeaderValue::from_static("TV>Android"),
    );
    map.insert(
        HeaderName::from_static("x-app-version"),
        HeaderValue::from_str(&api_version.major)
            .context("Invalid RTL Play x-app-version header.")?,
    );
    Ok(map)
}

/// Formats the RTLplay Android user-agent for an API version.
///
/// # Arguments
/// * `api_version` - Version components inserted into the user-agent.
///
/// # Returns
/// The RTLplay user-agent string.
fn rtlplay_user_agent(api_version: &RtlPlayApiVersion) -> String {
    format!(
        "RTL_PLAY/{}.{} (com.tapptic.rtl.tvi; build:{}; Android 30)",
        api_version.major, api_version.minor, api_version.build
    )
}

/// Reads a non-empty scalar from a nested JSON path.
///
/// # Arguments
/// * `payload` - JSON value to traverse.
/// * `path` - Object keys applied successively.
/// * `error_message` - Message used when a key or scalar is missing.
///
/// # Errors
/// Returns an error when traversal fails or the target is not a supported
/// non-empty scalar.
///
/// # Returns
/// The trimmed scalar as a string.
fn read_json_string(payload: &Value, path: &[&str], error_message: &str) -> Result<String> {
    let mut current = payload;

    for segment in path {
        current = current
            .get(*segment)
            .with_context(|| error_message.to_string())?;
    }

    if let Some(value) = json_scalar_to_string(current) {
        return Ok(value);
    }

    bail!(error_message.to_string())
}

/// Converts a string, number or boolean JSON scalar into a trimmed string.
///
/// # Arguments
/// * `value` - JSON value to convert.
///
/// # Returns
/// The non-empty trimmed scalar, or `None` for other value types or empty
/// strings.
fn json_scalar_to_string(value: &Value) -> Option<String> {
    let value = match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        _ => return None,
    };
    let value = value.trim();

    (!value.is_empty()).then(|| value.to_string())
}

mod authentication;
use authentication::*;
mod media;
use media::*;
