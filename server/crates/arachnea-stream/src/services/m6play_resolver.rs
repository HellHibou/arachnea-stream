mod authentication;
mod media;

use authentication::{fetch_upfront_token, get_or_login_session};
use media::{
    extract_chapters_from_video_payload, extract_manifest_url,
    extract_storyboard_from_video_payload, fetch_video_payload_json, select_best_asset_url,
};

use anyhow::{bail, Context, Result};
use arachnea_proxy::http::actions::{
    ProxyHttpActionConfig, RemoveHeader, ReplaceAll, REMOVE_HEADER_ACTION_HEADER,
};
use arachnea_proxy::http::proxy_service::proxied_url_with_countries;
use arachnea_proxy::PROXY_HEADER_PARAMETER_COUNTRIES;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

use arachnea_core::persistence::CredentialsStore;
use arachnea_scrapyfy::{ScraperAgregator, ScraperHttpConfig, ScraperQueryCollectionParameter};

use crate::services::player_resolver::{
    normalize_stream_kind, proxy_drm_today_license_request, save_drm_today_license_proxy_url,
    PlayerResolverEndpoints, PlayerStreamResolver, ProxiedStreamResponse, ResolvedPlayerStream,
};

/// M6/6play login endpoint.
const SIXPLAY_LOGIN_URL: &str = "https://login-gigya.m6.fr/accounts.login";

/// M6/6play browser login page endpoint.
const SIXPLAY_LOGIN_PAGE_URL: &str = "https://www.6play.fr/connexion";

/// M6/6play JavaScript bundle used for API-key discovery URL template.
const SIXPLAY_BUNDLE_URL_TEMPLATE: &str = "https://www.6play.fr/main-{}.bundle.js";

/// Fallback public API identifier when the M6/6play bundle lookup fails.
const SIXPLAY_API_KEY_FALLBACK: &str =
    "3_hH5KBv25qZTd_sURpixbQW6a4OsiIzIEF2Ei_2H7TXTGLJb_1Hr4THKZianCQhWK";

/// M6/6play JWT token endpoint.
const SIXPLAY_TOKEN_UUID_URL: &str = "https://front-auth.6cloud.fr/v2/platforms/m6group_web/getJwt";

/// M6/6play replay DRM token URL template.
const SIXPLAY_TOKEN_REPLAY_URL_TEMPLATE: &str = "https://drm.6cloud.fr/v1/customers/m6web/platforms/m6group_web/services/m6replay/users/{}/videos/{}/upfront-token";

/// M6/6play live DRM token URL template.
const SIXPLAY_TOKEN_LIVE_URL_TEMPLATE: &str = "https://drm.6cloud.fr/v1/customers/m6web/platforms/m6group_web/services/6play/users/{}/live/{}/upfront-token";

/// M6/6play video metadata URL template.
const SIXPLAY_VIDEO_JSON_URL_TEMPLATE: &str = "https://android.middleware.6play.fr/6play/v2/platforms/m6group_androidmob/services/6play/videos/{}?csa=6&with=clips,freemiumpacks";

/// M6/6play live metadata endpoint.
const SIXPLAY_LIVE_JSON_URL: &str =
    "https://android.middleware.6play.fr/6play/v2/platforms/m6group_androidmob/services/6play/live";

/// M6/6play DRM license endpoint.
const SIXPLAY_LICENSE_URL: &str =
    "https://lic.drmtoday.com/license-proxy-widevine/cenc/?specConform=true";

/// Client release identifier sent to the M6/6play API.
const SIXPLAY_CLIENT_RELEASE: &str = "5.103.3";

/// Customer identifier sent to the M6/6play DRM service.
const SIXPLAY_CUSTOMER_NAME: &str = "m6web";

/// JSONP callback name expected by the M6/6play login flow.
const SIXPLAY_CALLBACK_NAME: &str = "jsonp_arachnea";

/// Client device identifier sent to the M6/6play API.
const SIXPLAY_DEVICE_ID: &str = "_luid_arachnea";

/// Lifetime of cached M6/6play login sessions.
const SIXPLAY_SESSION_TTL: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// Total optional API-key lookup budget, shared by the page and bundle requests.
const SIXPLAY_API_KEY_LOOKUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Lifetime of a key extracted from the public web bundle.
const SIXPLAY_API_KEY_TTL: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// Delay before retrying web discovery after falling back to the bundled key.
const SIXPLAY_API_KEY_FALLBACK_TTL: std::time::Duration = std::time::Duration::from_secs(60);

/// Stable source identifier for M6/6play routing and credentials.
const M6PLAY_SERVICE_ID: &str = "m6play-fr";

/// HTTP status classified as a destination rejection for M6/6play.
const M6PLAY_PROXY_REJECTION_STATUS: u16 = 403;

/// M6/6play storyboard tile width in pixels.
const SIXPLAY_STORYBOARD_WIDTH: u32 = 200;

/// M6/6play storyboard tile height in pixels.
const SIXPLAY_STORYBOARD_HEIGHT: u32 = 112;

/// Number of columns in the M6/6play storyboard sprite layout.
const SIXPLAY_STORYBOARD_COLUMNS: u32 = 300;

/// Number of rows in the M6/6play storyboard sprite layout.
const SIXPLAY_STORYBOARD_ROWS: u32 = 1;

/// M6+ implementation of the generic protected playback resolver contract.
pub(crate) struct M6PlayResolver;

#[async_trait]
impl PlayerStreamResolver for M6PlayResolver {
    fn source_id(&self) -> &'static str {
        M6PLAY_SERVICE_ID
    }

    fn resolver_ids(&self) -> &'static [&'static str] {
        &["m6play-video", "m6play-live"]
    }

    async fn get_stream(
        &self,
        scraper_agregator: &ScraperAgregator,
        credentials_store: &dyn CredentialsStore,
        resolver: &str,
        target: &str,
        _service_parameters: &[ScraperQueryCollectionParameter],
        proxy_countries: &[String],
        endpoints: &PlayerResolverEndpoints,
    ) -> Result<ResolvedPlayerStream> {
        match resolver.trim() {
            "m6play-video" => {
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
            "m6play-live" => {
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
                M6PLAY_SERVICE_ID
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

/// Resolves an M6 replay video into a protected DASH player stream.
///
/// # Arguments
/// * `scraper_agregator` - Aggregator used to create the HTTP client.
/// * `credentials_store` - Store providing the M6 credentials.
/// * `video_id` - M6 video identifier; surrounding whitespace is ignored.
/// * `stream_kind` - Optional caller-selected stream kind.
/// * `proxy_countries` - Countries selected for outbound routing.
/// * `endpoints` - Public endpoint settings used to build proxy URLs.
///
/// # Errors
/// Returns an error when the identifier is empty, login or token retrieval
/// fails, or the payload has no usable DASH asset.
///
/// # Returns
/// The resolved stream with proxied manifest, license, storyboard and chapters.
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
        bail!("Missing 6play video identifier.");
    }

    let http_client = scraper_agregator.create_http_client(m6play_http_config(proxy_countries));
    let session = get_or_login_session(&http_client, credentials_store).await?;
    let upfront_token = fetch_upfront_token(&http_client, &session, normalized_video_id).await?;

    let video_payload = fetch_video_payload_json(&http_client, normalized_video_id).await?;
    let manifest_url = extract_manifest_url(&video_payload)?;

    let manifest_url = http_client
        .resolve_final_url_for_request(http::Method::GET, &manifest_url, &HashMap::new(), None)
        .await
        .unwrap_or(manifest_url);

    let storyboard = extract_storyboard_from_video_payload(&video_payload);
    let chapters = extract_chapters_from_video_payload(&video_payload);

    let stream_kind = normalize_stream_kind(stream_kind);
    let license_url = save_drm_today_license_proxy_url(
        endpoints,
        M6PLAY_SERVICE_ID,
        &upfront_token,
        &stream_kind,
        SIXPLAY_LICENSE_URL,
        None,
    );
    let stream_actions = stream_headers();

    Ok(ResolvedPlayerStream {
        stream_url: vec![proxied_url_with_countries(
            &manifest_url,
            endpoints.http_proxy_public_path.as_deref(),
            proxy_countries,
            &stream_actions,
            &[],
        )],
        manifest_type: Some("mpd".to_string()),
        license_url: Some(license_url),
        license_headers: HashMap::new(),
        storyboard,
        chapters,
        ..Default::default()
    })
}

/// Resolves an M6 live channel into a protected DASH player stream.
///
/// # Arguments
/// * `scraper_agregator` - Aggregator used to create the HTTP client.
/// * `credentials_store` - Store providing the M6 credentials.
/// * `channel_id` - M6 channel identifier; surrounding whitespace is ignored.
/// * `stream_kind` - Optional caller-selected stream kind.
/// * `proxy_countries` - Countries selected for outbound routing.
/// * `endpoints` - Public endpoint settings used to build proxy URLs.
///
/// # Errors
/// Returns an error when the identifier is empty, login or token retrieval
/// fails, or the live payload has no usable DASH asset.
///
/// # Returns
/// The resolved live stream with proxied manifest and license.
async fn resolve_live_stream(
    scraper_agregator: &ScraperAgregator,
    credentials_store: &dyn CredentialsStore,
    channel_id: &str,
    stream_kind: Option<String>,
    proxy_countries: &[String],
    endpoints: &PlayerResolverEndpoints,
) -> Result<ResolvedPlayerStream> {
    let normalized_channel = channel_id.trim();
    if normalized_channel.is_empty() {
        bail!("Missing 6play channel identifier.");
    }

    let http_client = scraper_agregator.create_http_client(m6play_http_config(proxy_countries));
    let session = get_or_login_session(&http_client, credentials_store).await?;

    // Map channel identifier to the API's live_item_id format.
    let live_item_id = match normalized_channel {
        "6ter" => "6T".to_string(),
        "m6" => "M6".to_string(),
        "w9" => "W9".to_string(),
        other => other.to_string(),
    };

    // Fetch upfront token for live stream.
    let token_target_id = format!("dashcenc_{}", live_item_id);
    let token_url = SIXPLAY_TOKEN_LIVE_URL_TEMPLATE
        .replacen("{}", &session.account_id, 1)
        .replacen("{}", &token_target_id, 1);

    let mut auth_headers = HashMap::new();
    auth_headers.insert(
        "x-customer-name".to_string(),
        SIXPLAY_CUSTOMER_NAME.to_string(),
    );
    auth_headers.insert(
        "x-client-release".to_string(),
        SIXPLAY_CLIENT_RELEASE.to_string(),
    );
    auth_headers.insert(
        "authorization".to_string(),
        format!("Bearer {}", session.login_token),
    );

    let token_payload = http_client
        .get_json_for_request(http::Method::GET, &token_url, &auth_headers, None)
        .await
        .context("Failed to fetch the 6play live upfront token.")?;

    let token = read_json_string(
        &token_payload,
        &["token"],
        "Missing 6play upfront token for live playback.",
    )?;

    // Fetch live JSON to obtain manifest URL.
    let live_url = format!(
        "{}?channel={}&with=service_display_images,nextdiffusion,extra_data",
        SIXPLAY_LIVE_JSON_URL, live_item_id
    );

    let mut generic_headers = HashMap::new();
    generic_headers.insert(
        "user-agent".to_string(),
        crate::PLAYER_DEFAULT_USER_AGENT.to_string(),
    );

    let live_json = http_client
        .get_json_for_request(http::Method::GET, &live_url, &generic_headers, None)
        .await?;

    let channel_array = live_json
        .get(live_item_id)
        .and_then(Value::as_array)
        .context("Missing live channel data in response.")?;

    let first = channel_array.first().context("Live channel array empty")?;

    let assets = first
        .get("live")
        .and_then(Value::as_object)
        .and_then(|obj| obj.get("assets"))
        .and_then(Value::as_array)
        .context("Missing live assets")?;

    let manifest_url = select_best_asset_url(assets, "delta_dashcenc_h264")
        .context("Missing live DASH DRM manifest")?;

    let stream_kind = normalize_stream_kind(stream_kind);
    let license_url = save_drm_today_license_proxy_url(
        endpoints,
        M6PLAY_SERVICE_ID,
        &token,
        &stream_kind,
        SIXPLAY_LICENSE_URL,
        None,
    );
    let stream_actions = stream_headers();

    Ok(ResolvedPlayerStream {
        stream_url: vec![proxied_url_with_countries(
            &manifest_url,
            endpoints.http_proxy_public_path.as_deref(),
            proxy_countries,
            &stream_actions,
            &[],
        )],
        manifest_type: Some("mpd".to_string()),
        license_url: Some(license_url),
        license_headers: HashMap::new(),
        ..Default::default()
    })
}

/// Builds the HTTP configuration used for M6 requests.
///
/// # Arguments
/// * `proxy_countries` - Countries to use for outbound routing.
///
/// # Returns
/// A configuration treating HTTP 403 as a rejection and grouping M6 domains.
fn m6play_http_config(proxy_countries: &[String]) -> ScraperHttpConfig {
    ScraperHttpConfig {
        proxy_rejection_statuses: vec![M6PLAY_PROXY_REJECTION_STATUS],
        ..ScraperHttpConfig::default()
            .proxy_countries(proxy_countries.to_vec())
            .proxy_domain_group(
                ["m6.fr", "6play.fr", "6cloud.fr", "bedrock.tech"]
                    .into_iter()
                    .map(str::to_string),
            )
    }
}

/// Builds proxy actions that rewrite M6 DASH segment paths.
///
/// # Returns
/// Actions removing internal proxy headers on redirects and rewriting
/// initialization and media paths.
fn stream_headers() -> Vec<ProxyHttpActionConfig> {
    vec![
        RemoveHeader::on_http302([
            PROXY_HEADER_PARAMETER_COUNTRIES,
            REMOVE_HEADER_ACTION_HEADER,
        ]),
        ReplaceAll::new(
            r#"initialization="/m6web/"#,
            r#"initialization="{proxy}/{base_url}/m6web/"#,
            None,
        ),
        ReplaceAll::new(
            r#"media="/m6web/"#,
            r#"media="{proxy}/{base_url}/m6web/"#,
            None,
        ),
    ]
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
