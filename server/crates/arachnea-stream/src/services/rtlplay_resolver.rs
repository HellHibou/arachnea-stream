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
    Chapter, PlayerResolverEndpoints, PlayerStreamResolver, ProxiedStreamResponse,
    ResolvedPlayerStream,
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

static RTLPLAY_SESSION_CACHE: OnceLock<Mutex<HashMap<String, CachedRtlPlaySession>>> =
    OnceLock::new();
static RTLPLAY_LOGIN_LOCK: OnceLock<AsyncMutex<()>> = OnceLock::new();
static NEXT_DATA_REGEX: OnceLock<Regex> = OnceLock::new();

#[derive(Clone)]
struct RtlPlaySession {
    cookies: HashMap<String, String>,
}

struct CachedRtlPlaySession {
    session: RtlPlaySession,
    expires_at: Instant,
}

struct ResolvedRtlPlayVideo {
    manifest_url: String,
    license_url: Option<String>,
    license_token: Option<String>,
    storyboard_vtt_url: Option<String>,
    chapters: Vec<Chapter>,
}

struct RtlPlayApiVersion {
    major: String,
    minor: String,
    build: String,
}

impl RtlPlayApiVersion {
    fn from_parameters(parameters: &[ScraperQueryCollectionParameter]) -> Result<Self> {
        Ok(Self {
            major: required_collection_parameter(parameters, "api_version_major")?,
            minor: required_collection_parameter(parameters, "api_version_minor")?,
            build: required_collection_parameter(parameters, "api_version_build")?,
        })
    }
}

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
        resolver: &str,
        target: &str,
        service_parameters: &[ScraperQueryCollectionParameter],
        _proxy_countries: &[String],
        endpoints: &PlayerResolverEndpoints,
    ) -> Result<ResolvedPlayerStream> {
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

fn rtlplay_http_config() -> ScraperHttpConfig {
    ScraperHttpConfig {
        max_redirects: Some(RTLPLAY_AUTH_MAX_REDIRECTS),
        ..Default::default()
    }
}

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

fn rtlplay_user_agent(api_version: &RtlPlayApiVersion) -> String {
    format!(
        "RTL_PLAY/{}.{} (com.tapptic.rtl.tvi; build:{}; Android 30)",
        api_version.major, api_version.minor, api_version.build
    )
}

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
