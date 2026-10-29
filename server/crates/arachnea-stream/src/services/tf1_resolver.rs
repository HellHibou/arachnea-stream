use anyhow::{bail, Context, Result};
use arachnea_proxy::{
    core::http::response_classifier::{
        ProxyBodyRequirement, ProxyBodyState, ProxyResponseClassifier, ProxyResponseDecision,
        ProxyResponsePolicy, ProxyResponseView, MAX_PROXY_RESPONSE_PREFIX,
    },
    http::{actions::ReplaceAll, proxy_service::proxied_url_with_countries_and_policy},
};
use async_trait::async_trait;
use rand::{distr::Alphanumeric, Rng};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use arachnea_core::persistence::CredentialsStore;
use arachnea_scrapyfy::{ArachneaHttpError, HttpClient, ScraperAgregator, ScraperHttpConfig};

use crate::services::player_resolver::{
    normalize_stream_kind, Chapter, PlayerResolverEndpoints, PlayerStreamRequest,
    PlayerStreamResolver, ProxiedStreamResponse, ResolvedPlayerImageTitle, ResolvedPlayerStream,
};

use crate::PLAYER_ERROR_BODY_PREVIEW_LENGTH;

/// Stable source identifier for TF1 routing and credentials.
const TF1_SERVICE_ID: &str = "tf1-fr";

/// Website origin used by the TF1 resolver.
const TF1_BASE_URL: &str = "https://www.tf1.fr";

/// Public Gigya application identifier used by TF1.
const TF1_GIGYA_API_KEY: &str =
    "3_hWgJdARhz_7l1oOp3a8BDLoR9cuWZpUaKG4aqF7gum9_iK3uTZ2VlDBl8ANf8FVk";

/// TF1 Gigya bootstrap endpoint.
const TF1_BOOTSTRAP_URL: &str = "https://compte.tf1.fr/accounts.webSdkBootstrap";

/// TF1 login endpoint.
const TF1_LOGIN_URL: &str = "https://compte.tf1.fr/accounts.login";

/// TF1 Gigya token exchange endpoint.
const TF1_TOKEN_URL: &str = "https://www.tf1.fr/token/gigya/web";

/// TF1 media information URL template.
const TF1_MEDIA_INFO_URL_TEMPLATE: &str = "https://mediainfo.tf1.fr/mediainfocombo/{}";

/// TF1 fallback Widevine license URL template.
const TF1_FALLBACK_LICENSE_URL_TEMPLATE: &str = "https://drm-wide.tf1.fr/proxy?id={}";

/// Stream-kind identifier selecting the TF1 license proxy.
const TF1_PROXY_STREAM_KIND: &str = "widevine-license-proxy";

/// HTTP status classified as a destination rejection for TF1.
const TF1_PROXY_REJECTION_STATUS: u16 = 403;

/// Lifetime of cached TF1 login sessions.
const TF1_SESSION_TTL: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// Lifetime of cached TF1 license proxy tokens.
const TF1_LICENSE_TTL: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// Process-wide cache of authenticated TF1 sessions keyed by login.
static TF1_SESSION_CACHE: OnceLock<Mutex<HashMap<String, CachedTf1Session>>> = OnceLock::new();
/// Process-wide cache of license proxy tokens and their upstream requests.
static TF1_LICENSE_CACHE: OnceLock<Mutex<HashMap<String, CachedTf1License>>> = OnceLock::new();
/// Serializes cache misses during the TF1 login flow.
static TF1_LOGIN_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

/// Authenticated TF1 web token used for mediainfo requests.
#[derive(Clone)]
struct Tf1Session {
    token: String,
}

/// Cached TF1 session with its expiry instant.
struct CachedTf1Session {
    session: Tf1Session,
    expires_at: Instant,
}

/// Cached license proxy request with its routing metadata and expiry.
#[derive(Clone)]
struct CachedTf1License {
    license_url: String,
    headers: HashMap<String, String>,
    stream_kind: String,
    proxy_countries: Vec<String>,
    expires_at: Instant,
}

/// TF1+ implementation of the generic protected playback resolver contract.
pub(crate) struct Tf1Resolver;

#[async_trait]
impl PlayerStreamResolver for Tf1Resolver {
    fn source_id(&self) -> &'static str {
        TF1_SERVICE_ID
    }

    fn resolver_ids(&self) -> &'static [&'static str] {
        &["tf1-video", "tf1-live"]
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
            proxy_countries,
        } = request;
        match resolver.trim() {
            "tf1-video" => {
                resolve_replay_stream(
                    scraper_agregator,
                    credentials_store,
                    target,
                    None,
                    proxy_countries,
                    endpoints,
                )
                .await
            }
            "tf1-live" => {
                resolve_live_stream(
                    scraper_agregator,
                    credentials_store,
                    target,
                    None,
                    proxy_countries,
                    endpoints,
                )
                .await
            }
            kind => bail!(
                "Unsupported player resolver `{}` for source `{}`.",
                kind,
                TF1_SERVICE_ID
            ),
        }
    }

    async fn get_drm_license(
        &self,
        scraper_agregator: &ScraperAgregator,
        stream_token: &str,
        body: &[u8],
    ) -> Result<ProxiedStreamResponse> {
        proxy_tf1_license_request(scraper_agregator, stream_token, body).await
    }
}

/// Resolves a TF1 replay video into a protected player stream.
///
/// # Arguments
/// * `scraper_agregator` - Aggregator used to create the HTTP clients.
/// * `credentials_store` - Store providing the TF1 credentials.
/// * `video_id` - TF1 video identifier; surrounding whitespace is ignored.
/// * `stream_kind` - Optional caller-selected stream kind.
/// * `proxy_countries` - Countries initially selected for outbound routing.
/// * `endpoints` - Public endpoint settings used to build proxy URLs.
///
/// # Errors
/// Returns an error when the identifier is empty or the replay negotiation
/// fails.
///
/// # Returns
/// The resolved replay stream.
async fn resolve_replay_stream(
    scraper_agregator: &ScraperAgregator,
    credentials_store: &dyn CredentialsStore,
    video_id: &str,
    stream_kind: Option<String>,
    proxy_countries: &[String],
    endpoints: &PlayerResolverEndpoints,
) -> Result<ResolvedPlayerStream> {
    let normalized_video_id = video_id.trim();
    if normalized_video_id.is_empty() {
        bail!("Missing TF1 video identifier.");
    }

    resolve_replay_stream_with_config(
        scraper_agregator,
        credentials_store,
        normalized_video_id,
        stream_kind,
        tf1_http_config(proxy_countries),
        proxy_countries,
        endpoints,
    )
    .await
}

/// Negotiates TF1 replay playback with a prepared HTTP configuration.
///
/// When mediainfo reports a different non-empty set of allowed countries, a
/// second client is created for those countries and mediainfo is retried only
/// if the first delivery was rejected.
///
/// # Arguments
/// * `scraper_agregator` - Aggregator used to create HTTP clients.
/// * `credentials_store` - Store providing the TF1 credentials.
/// * `normalized_video_id` - Trimmed TF1 video identifier.
/// * `stream_kind` - Optional caller-selected stream kind.
/// * `http_config` - Initial HTTP configuration.
/// * `proxy_countries` - Countries represented by `http_config`.
/// * `endpoints` - Public endpoint settings used to build proxy URLs.
///
/// # Errors
/// Returns an error when login, mediainfo retrieval or stream construction
/// fails.
///
/// # Returns
/// The resolved replay stream.
async fn resolve_replay_stream_with_config(
    scraper_agregator: &ScraperAgregator,
    credentials_store: &dyn CredentialsStore,
    normalized_video_id: &str,
    stream_kind: Option<String>,
    http_config: ScraperHttpConfig,
    proxy_countries: &[String],
    endpoints: &PlayerResolverEndpoints,
) -> Result<ResolvedPlayerStream> {
    tracing::info!(
        video_id = normalized_video_id,
        requested_proxy_countries = ?proxy_countries,
        configured_proxy_countries = ?http_config.proxy_countries,
        "starting TF1 replay negotiation"
    );
    let http_client = tf1_http_client(scraper_agregator, http_config);
    let session = get_or_login_session(&http_client, credentials_store).await?;
    let media_info = fetch_media_info(&http_client, &session, normalized_video_id, false).await?;

    let media_proxy_countries = extract_media_proxy_countries(&media_info);
    tracing::info!(
        video_id = normalized_video_id,
        requested_proxy_countries = ?proxy_countries,
        media_proxy_countries = ?media_proxy_countries,
        delivery_rejected = delivery_rejected(&media_info),
        countries_changed = media_proxy_countries != proxy_countries,
        retry_mediainfo = !media_proxy_countries.is_empty()
            && media_proxy_countries != proxy_countries
            && delivery_rejected(&media_info),
        "evaluated TF1 replay geo-routing decision"
    );
    if !media_proxy_countries.is_empty() && media_proxy_countries != proxy_countries {
        let media_http_client =
            tf1_http_client(scraper_agregator, tf1_http_config(&media_proxy_countries));
        let media_info = if delivery_rejected(&media_info) {
            fetch_media_info(&media_http_client, &session, normalized_video_id, false).await?
        } else {
            media_info
        };

        return build_resolved_player_stream(
            &media_http_client,
            normalized_video_id,
            &media_info,
            stream_kind,
            &media_proxy_countries,
            endpoints,
        )
        .await;
    }

    build_resolved_player_stream(
        &http_client,
        normalized_video_id,
        &media_info,
        stream_kind,
        proxy_countries,
        endpoints,
    )
    .await
}

/// Resolves a TF1 live channel into a protected player stream.
///
/// # Arguments
/// * `scraper_agregator` - Aggregator used to create the HTTP clients.
/// * `credentials_store` - Store providing the TF1 credentials.
/// * `channel_id` - TF1 live channel identifier; surrounding whitespace is ignored.
/// * `stream_kind` - Optional caller-selected stream kind.
/// * `proxy_countries` - Countries initially selected for outbound routing.
/// * `endpoints` - Public endpoint settings used to build proxy URLs.
///
/// # Errors
/// Returns an error when the channel identifier is empty or the live
/// negotiation fails.
///
/// # Returns
/// The resolved live stream.
async fn resolve_live_stream(
    scraper_agregator: &ScraperAgregator,
    credentials_store: &dyn CredentialsStore,
    channel_id: &str,
    stream_kind: Option<String>,
    proxy_countries: &[String],
    endpoints: &PlayerResolverEndpoints,
) -> Result<ResolvedPlayerStream> {
    let normalized_channel_id = channel_id.trim();
    if normalized_channel_id.is_empty() {
        bail!("Missing TF1 live channel identifier.");
    }

    resolve_live_stream_with_config(
        scraper_agregator,
        credentials_store,
        normalized_channel_id,
        stream_kind,
        tf1_http_config(proxy_countries),
        proxy_countries,
        endpoints,
    )
    .await
}

/// Negotiates TF1 live playback with a prepared HTTP configuration.
///
/// The live media identifier is derived as `L_` followed by the uppercase
/// channel identifier. Country switching follows the same retry policy as
/// replay negotiation.
///
/// # Arguments
/// * `scraper_agregator` - Aggregator used to create HTTP clients.
/// * `credentials_store` - Store providing the TF1 credentials.
/// * `normalized_channel_id` - Trimmed TF1 live channel identifier.
/// * `stream_kind` - Optional caller-selected stream kind.
/// * `http_config` - Initial HTTP configuration.
/// * `proxy_countries` - Countries represented by `http_config`.
/// * `endpoints` - Public endpoint settings used to build proxy URLs.
///
/// # Errors
/// Returns an error when login, mediainfo retrieval or stream construction
/// fails.
///
/// # Returns
/// The resolved live stream.
async fn resolve_live_stream_with_config(
    scraper_agregator: &ScraperAgregator,
    credentials_store: &dyn CredentialsStore,
    normalized_channel_id: &str,
    stream_kind: Option<String>,
    http_config: ScraperHttpConfig,
    proxy_countries: &[String],
    endpoints: &PlayerResolverEndpoints,
) -> Result<ResolvedPlayerStream> {
    tracing::info!(
        channel_id = normalized_channel_id,
        requested_proxy_countries = ?proxy_countries,
        configured_proxy_countries = ?http_config.proxy_countries,
        "starting TF1 live negotiation"
    );
    let http_client = tf1_http_client(scraper_agregator, http_config);
    let session = get_or_login_session(&http_client, credentials_store).await?;
    let live_video_id = format!("L_{}", normalized_channel_id.to_uppercase());
    let media_info = fetch_media_info(&http_client, &session, &live_video_id, true).await?;

    let media_proxy_countries = extract_media_proxy_countries(&media_info);
    tracing::info!(
        video_id = %live_video_id,
        requested_proxy_countries = ?proxy_countries,
        media_proxy_countries = ?media_proxy_countries,
        delivery_rejected = delivery_rejected(&media_info),
        countries_changed = media_proxy_countries != proxy_countries,
        retry_mediainfo = !media_proxy_countries.is_empty()
            && media_proxy_countries != proxy_countries
            && delivery_rejected(&media_info),
        "evaluated TF1 live geo-routing decision"
    );
    if !media_proxy_countries.is_empty() && media_proxy_countries != proxy_countries {
        let media_http_client =
            tf1_http_client(scraper_agregator, tf1_http_config(&media_proxy_countries));
        let media_info = if delivery_rejected(&media_info) {
            fetch_media_info(&media_http_client, &session, &live_video_id, true).await?
        } else {
            media_info
        };

        return build_resolved_player_stream(
            &media_http_client,
            &live_video_id,
            &media_info,
            stream_kind,
            &media_proxy_countries,
            endpoints,
        )
        .await;
    }

    build_resolved_player_stream(
        &http_client,
        &live_video_id,
        &media_info,
        stream_kind,
        proxy_countries,
        endpoints,
    )
    .await
}

/// Builds the HTTP configuration used for TF1 requests.
///
/// # Arguments
/// * `proxy_countries` - Countries to use for outbound proxy routing.
///
/// # Returns
/// A configuration that treats HTTP 403 as a destination rejection.
fn tf1_http_config(proxy_countries: &[String]) -> ScraperHttpConfig {
    ScraperHttpConfig {
        proxy_rejection_statuses: vec![TF1_PROXY_REJECTION_STATUS],
        ..ScraperHttpConfig::default().proxy_countries(proxy_countries.to_vec())
    }
}

/// Creates an HTTP client with the TF1 mediainfo response classifier.
///
/// # Arguments
/// * `scraper_agregator` - Aggregator used to create the client.
/// * `http_config` - HTTP configuration for the client.
///
/// # Returns
/// A client that can detect TF1 geo restrictions embedded in response bodies.
fn tf1_http_client(
    scraper_agregator: &ScraperAgregator,
    http_config: ScraperHttpConfig,
) -> HttpClient {
    scraper_agregator
        .create_http_client(http_config)
        .with_proxy_response_classifier(ProxyResponsePolicy(Arc::new(
            Tf1MediainfoResponseClassifier,
        )))
}

/// Builds a resolved player stream from a TF1 mediainfo payload.
///
/// # Arguments
/// * `http_client` - Client used to resolve the final manifest URL.
/// * `video_id` - TF1 video or live content identifier.
/// * `media_info` - Decoded TF1 mediainfo response.
/// * `stream_kind` - Optional caller-selected stream kind.
/// * `proxy_countries` - Countries used for storyboard and license routing.
/// * `endpoints` - Public endpoint settings used to build proxy URLs.
///
/// # Errors
/// Returns an error when the delivery payload is missing, playback was
/// rejected, the manifest URL is absent, or manifest resolution fails with a
/// proxy error.
///
/// # Returns
/// The resolved stream with manifest, license proxy, artwork and chapters.
async fn build_resolved_player_stream(
    http_client: &HttpClient,
    video_id: &str,
    media_info: &Value,
    stream_kind: Option<String>,
    proxy_countries: &[String],
    endpoints: &PlayerResolverEndpoints,
) -> Result<ResolvedPlayerStream> {
    let delivery = media_info
        .get("delivery")
        .context("Missing TF1 delivery payload.")?;
    let delivery_code = delivery
        .get("code")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if delivery_code >= 400 {
        let message = delivery
            .get("message")
            .or_else(|| delivery.get("error"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .or_else(|| {
                media_info
                    .pointer("/media/error_desc")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
            })
            .map(String::from)
            .unwrap_or(format!(
                "TF1 rejected playback for this video (http error {}).",
                delivery_code
            ));
        bail!(message.to_string());
    }

    let raw_manifest_url = read_json_string(
        delivery,
        &["url"],
        "Missing TF1 media manifest URL in mediainfo response.",
    )?;
    let manifest_url_result = http_client
        .resolve_final_url_for_request(http::Method::GET, &raw_manifest_url, &HashMap::new(), None)
        .await;
    let manifest_url = match manifest_url_result {
        Ok(url) => url,
        Err(error) if is_proxy_error(&error) => return Err(error),
        Err(_) => raw_manifest_url,
    };
    let manifest_type = manifest_type_from_url(&manifest_url);

    let fallback_license_url = TF1_FALLBACK_LICENSE_URL_TEMPLATE.replace("{}", video_id.trim());
    let license_url = delivery
        .pointer("/drms/0/url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback_license_url.as_str())
        .to_string();
    let license_headers = extract_license_headers(delivery);
    let stream_kind = normalize_stream_kind(stream_kind);
    let proxy_url = save_tf1_license_proxy_url(
        endpoints,
        &license_url,
        &license_headers,
        &stream_kind,
        proxy_countries,
    );

    let title = media_info
        .pointer("/media/title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(String::from);

    let image_title = media_info
        .pointer("/media/preview")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|url| ResolvedPlayerImageTitle {
            link: url.to_string(),
        });

    let storyboard_vtt_url = media_info
        .pointer("/media/sb")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|url| proxied_tf1_storyboard_vtt_url(endpoints, url, proxy_countries));

    let chapters = extract_chapters(media_info);

    Ok(ResolvedPlayerStream {
        stream_url: vec![proxied_url_with_countries_and_policy(
            &manifest_url,
            endpoints.http_proxy_public_path.as_deref(),
            &[], // proxy_countries, // No proxies fro now but keep proxy_countries commented and use it if we want to proxy the manifest in the future
            None,
            &[],
            &[],
            &[],
        )],
        manifest_type: Some(manifest_type),
        license_url: Some(proxy_url),
        license_headers: HashMap::new(),
        title,
        image_title,
        storyboard_vtt_url,
        chapters,
        ..Default::default()
    })
}

/// Extracts intro and outro chapters from a TF1 mediainfo payload.
///
/// # Arguments
/// * `media_info` - Decoded TF1 mediainfo response.
///
/// # Returns
/// The available chapters, or `None` when neither marker pair is valid.
fn extract_chapters(media_info: &Value) -> Option<Vec<Chapter>> {
    let chapters = [
        extract_chapter(media_info, "inGD", "outGD", "intro"),
        extract_chapter(media_info, "inGF", "outGF", "outro"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

    (!chapters.is_empty()).then_some(chapters)
}

/// Extracts one chapter from a TF1 marker pair.
///
/// Marker values are converted from milliseconds to seconds.
///
/// # Arguments
/// * `media_info` - Decoded TF1 mediainfo response.
/// * `start_marker` - Name of the start marker under `/media/markers`.
/// * `end_marker` - Name of the end marker under `/media/markers`.
/// * `chapter_type` - Chapter type assigned to the result.
///
/// # Returns
/// The chapter, or `None` when a marker is absent, non-positive or the end
/// does not follow the start.
fn extract_chapter(
    media_info: &Value,
    start_marker: &str,
    end_marker: &str,
    chapter_type: &str,
) -> Option<Chapter> {
    let start = media_info
        .pointer(&format!("/media/markers/{start_marker}"))
        .and_then(Value::as_f64)?;
    let end = media_info
        .pointer(&format!("/media/markers/{end_marker}"))
        .and_then(Value::as_f64)?;

    if start <= 0.0 || end <= 0.0 || end <= start {
        return None;
    }

    Some(Chapter {
        start: start / 1000.0,
        end: end / 1000.0,
        title: None,
        chapter_type: chapter_type.to_string(),
    })
}

/// Builds a proxied storyboard VTT URL that rewrites relative sprite paths.
///
/// # Arguments
/// * `endpoints` - Public endpoint settings used to build the proxy URL.
/// * `vtt_url` - Absolute storyboard VTT URL.
/// * `proxy_countries` - Countries to use for outbound proxy routing.
///
/// # Returns
/// The public proxy URL for the storyboard.
fn proxied_tf1_storyboard_vtt_url(
    endpoints: &PlayerResolverEndpoints,
    vtt_url: &str,
    proxy_countries: &[String],
) -> String {
    let actions = [ReplaceAll::config(
        r"(?m)^\s*(/[^#\r\n]+)(#xywh=\d+,\d+,\d+,\d+)",
        "{base_url}$1$2",
        None,
    )];

    proxied_url_with_countries_and_policy(
        vtt_url,
        endpoints.http_proxy_public_path.as_deref(),
        proxy_countries,
        None,
        &[TF1_PROXY_REJECTION_STATUS],
        &actions,
        &[],
    )
}

/// Extracts normalized license headers from `drms[0].h`.
///
/// Header names are lowercased and default to `authorization` when absent.
///
/// # Arguments
/// * `delivery` - TF1 delivery object containing the DRM metadata.
///
/// # Returns
/// A header map, possibly empty when no valid headers are present.
fn extract_license_headers(delivery: &Value) -> HashMap<String, String> {
    let Some(headers) = delivery.pointer("/drms/0/h").and_then(Value::as_array) else {
        return HashMap::new();
    };

    let mut result = HashMap::new();
    for header in headers {
        let value = header
            .get("v")
            .or_else(|| header.get("value"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let Some(value) = value else {
            continue;
        };

        let name = header
            .get("n")
            .or_else(|| header.get("name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or("authorization");
        result.insert(name.to_ascii_lowercase(), value.to_string());
    }

    result
}

/// Determines the manifest type from its URL.
///
/// # Arguments
/// * `url` - Manifest URL.
///
/// # Returns
/// `m3u8` for URLs containing that extension, `mpd` otherwise.
fn manifest_type_from_url(url: &str) -> String {
    if url.trim().to_ascii_lowercase().contains(".m3u8") {
        return "m3u8".to_string();
    }

    "mpd".to_string()
}

/// Reads a non-empty string or integer from a nested JSON path.
///
/// # Arguments
/// * `payload` - JSON value to traverse.
/// * `path` - Object keys applied successively.
/// * `error_message` - Message used when a key or valid scalar is missing.
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

/// Produces a bounded preview of an HTTP error body.
///
/// # Arguments
/// * `body` - Raw response body.
///
/// # Returns
/// A trimmed preview capped by `PLAYER_ERROR_BODY_PREVIEW_LENGTH`, with an
/// ellipsis when the original body is longer, or `<empty body>` when blank.
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

mod authentication;
use authentication::*;
mod media_info;
use media_info::*;
mod license;
use license::*;
