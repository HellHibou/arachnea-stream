mod authentication;
mod payload;
mod playback;

use authentication::{fetch_bff_json, get_or_login_session, CognitoSession};
use payload::{describe_sections, describe_value};
use playback::{extract_playback, resolved_stream};

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

use arachnea_core::persistence::CredentialsStore;
use arachnea_scrapyfy::{HttpClient, ScraperAgregator, ScraperQueryCollectionParameter};

use crate::services::player_resolver::{
    proxy_drm_today_license_request, PlayerResolverEndpoints, PlayerStreamResolver,
    ProxiedStreamResponse, ResolvedPlayerStream,
};

use crate::PLAYER_DEFAULT_USER_AGENT as USER_AGENT;

/// Stable source identifier for RMC/BFM Play routing and credentials.
const SERVICE_ID: &str = "rmcbfmplay-fr";

/// RMC/BFM Play authentication gateway endpoint.
const GATEWAY_URL: &str = "https://connect.rmcbfm.com/api/gateway";

/// Public client identifier used by the RMC/BFM Play authentication gateway.
const CONNECT_CLIENT_ID: &str = "1lof3c1vu7mscui8sfaal9qq2j";

/// RMC/BFM Play backend-for-frontend endpoint.
const BFF_BASE_URL: &str = "https://bff.rmcplus.fr/v1";

/// Client model identifier sent to the RMC/BFM Play backend-for-frontend API.
const BFF_MODEL: &str = "web";

/// Lifetime of cached RMC/BFM Play login sessions.
const SESSION_TTL: std::time::Duration = std::time::Duration::from_secs(45 * 60);

/// RMC+ implementation of the authenticated playback contract.
///
/// Login goes through the Connect gateway (`sign_in`, Cognito) and playback
/// metadata comes from the catalogue BFF, whose `player` composant only
/// exposes `video`/`drm` to authenticated callers. The legacy SFR CAS stack
/// (`sso.rmcbfmplay.com`, `heimdall`/`gaia`/`sekai`/`asgard`) is retired: the
/// CAS endpoint no longer answers.
pub(crate) struct RmcBfmPlayResolver;

#[async_trait]
impl PlayerStreamResolver for RmcBfmPlayResolver {
    fn source_id(&self) -> &'static str {
        SERVICE_ID
    }

    fn resolver_ids(&self) -> &'static [&'static str] {
        &["rmcbfmplay-video", "rmcbfmplay-live"]
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
        let target = target.trim();
        if target.is_empty() {
            bail!("Missing RMC BFM Play playback target.");
        }
        let http_client = scraper_agregator.create_http_client(Default::default());
        let session = get_or_login_session(&http_client, credentials_store).await?;
        match resolver.trim() {
            "rmcbfmplay-video" => {
                resolve_replay(&http_client, &session, target, proxy_countries, endpoints).await
            }
            "rmcbfmplay-live" => {
                resolve_live(&http_client, &session, target, proxy_countries, endpoints).await
            }
            kind => bail!("Unsupported player resolver `{kind}` for source `{SERVICE_ID}`."),
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

/// Resolves an RMC/BFM Play replay content identifier.
///
/// # Arguments
/// * `http_client` - HTTP client used for the BFF request.
/// * `session` - Authenticated Cognito session.
/// * `content_id` - Content identifier or BFF page URL.
/// * `proxy_countries` - Countries selected for outbound routing.
/// * `endpoints` - Public endpoint settings used to build proxy URLs.
///
/// # Errors
/// Returns an error when the identifier is invalid or the composant has no
/// usable video payload.
///
/// # Returns
/// The resolved replay stream.
async fn resolve_replay(
    http_client: &HttpClient,
    session: &CognitoSession,
    content_id: &str,
    proxy_countries: &[String],
    endpoints: &PlayerResolverEndpoints,
) -> Result<ResolvedPlayerStream> {
    let url = format!(
        "{BFF_BASE_URL}/composant/contenu/{}?source=cms&type=player&model={BFF_MODEL}",
        urlencoding::encode(&normalize_content_id(content_id)?)
    );
    resolve_composant_video(http_client, session, &url, proxy_countries, endpoints).await
}

/// Resolves an RMC/BFM Play live channel.
///
/// Embedded section videos are preferred; otherwise every player composant
/// endpoint from the live page is attempted in order.
///
/// # Arguments
/// * `http_client` - HTTP client used for the BFF requests.
/// * `session` - Authenticated Cognito session.
/// * `channel_id` - Channel identifier or page URL.
/// * `proxy_countries` - Countries selected for outbound routing.
/// * `endpoints` - Public endpoint settings used to build proxy URLs.
///
/// # Errors
/// Returns an error when the identifier is invalid, no composant is exposed,
/// or every candidate fails.
///
/// # Returns
/// The resolved live stream.
async fn resolve_live(
    http_client: &HttpClient,
    session: &CognitoSession,
    channel_id: &str,
    proxy_countries: &[String],
    endpoints: &PlayerResolverEndpoints,
) -> Result<ResolvedPlayerStream> {
    let page_url = format!(
        "{BFF_BASE_URL}/page?model={BFF_MODEL}&page_type=direct&page_id={}",
        urlencoding::encode(&normalize_content_id(channel_id)?)
    );
    let page = fetch_bff_json(http_client, session, &page_url, false).await?;
    // Live pages sometimes inline the player `video` object directly in a
    // section instead of exposing a `composant` endpoint: try embedded video
    // first, then the composant endpoints like the replay flow.
    if let Some(sections) = page.pointer("/sections").and_then(Value::as_array) {
        for section in sections {
            if let Some(video) = section.pointer("/video").filter(|video| video.is_object()) {
                let parts = extract_playback(http_client, video).await?;
                return resolved_stream(parts, proxy_countries, endpoints);
            }
        }
    }
    let candidates = composant_endpoints(&page);
    tracing::info!(
        channel_id,
        candidate_count = candidates.len(),
        section_detail = %describe_sections(&page),
        "RMC BFM Play live page scanned"
    );
    if candidates.is_empty() {
        bail!(
            "RMC BFM Play did not return a player composant for live channel `{channel_id}` ({})",
            describe_sections(&page)
        );
    }
    let mut failures = Vec::new();
    for url in &candidates {
        match resolve_composant_video(http_client, session, url, proxy_countries, endpoints).await {
            Ok(stream) => return Ok(stream),
            Err(error) => failures.push(format!("{url}: {error:#}")),
        }
    }
    bail!(
        "RMC BFM Play did not return video metadata for live channel `{channel_id}`: {}",
        failures.join("; ")
    );
}

/// Shared replay/live tail: fetches one `player` composant, extracts its
/// authenticated `video` payload, and builds the proxied stream.
///
/// # Arguments
/// * `http_client` - HTTP client used for the BFF request.
/// * `session` - Authenticated Cognito session.
/// * `url` - Player composant URL.
/// * `proxy_countries` - Countries selected for outbound routing.
/// * `endpoints` - Public endpoint settings used to build proxy URLs.
///
/// # Errors
/// Returns an error when the composant has no video payload or playback parts
/// cannot be extracted.
///
/// # Returns
/// The resolved stream built from the composant video.
async fn resolve_composant_video(
    http_client: &HttpClient,
    session: &CognitoSession,
    url: &str,
    proxy_countries: &[String],
    endpoints: &PlayerResolverEndpoints,
) -> Result<ResolvedPlayerStream> {
    let composant = fetch_bff_json(http_client, session, url, true).await?;
    let video = composant
        .pointer("/video")
        .filter(|video| video.is_object())
        .with_context(|| {
            format!(
                "RMC BFM Play did not return video metadata for `{url}` ({})",
                describe_value(&composant)
            )
        })?;
    let parts = extract_playback(http_client, video).await?;
    resolved_stream(parts, proxy_countries, endpoints)
}

/// Builds the headers used for BFF playback requests.
///
/// # Returns
/// JSON acceptance headers with the resolver user-agent.
fn playback_headers() -> HashMap<String, String> {
    HashMap::from([
        ("user-agent".to_string(), USER_AGENT.to_string()),
        (
            "accept".to_string(),
            "application/json, text/plain, */*".to_string(),
        ),
    ])
}

/// Normalizes a playback target to a BFF content id.
///
/// Callers pass either the raw id (catalogue-extracted targets) or the full
/// BFF page URL (the frontend calls `get_live` with the entry `link` and
/// `get_players` with the episode page URL). Both forms resolve to the
/// `page_id` query value when present.
///
/// # Arguments
/// * `target` - Raw content identifier or BFF page URL.
///
/// # Errors
/// Returns an error when the trimmed target is empty.
///
/// # Returns
/// The decoded `page_id` when present, otherwise the trimmed target.
fn normalize_content_id(target: &str) -> Result<String> {
    let target = target.trim();
    if target.is_empty() {
        bail!("Missing RMC BFM Play playback target.");
    }
    if let Some(query) = target.split_once('?').map(|(_, query)| query) {
        for pair in query.split('&') {
            let Some((name, value)) = pair.split_once('=') else {
                continue;
            };
            if name.trim() != "page_id" {
                continue;
            }
            let page_id = urlencoding::decode(value.trim())
                .map(|value| value.into_owned())
                .unwrap_or_else(|_| value.trim().to_string());
            if !page_id.trim().is_empty() {
                return Ok(page_id);
            }
        }
    }
    Ok(target.to_string())
}

/// Collects absolute player composant URLs from a BFF page.
///
/// # Arguments
/// * `page` - Decoded BFF page payload.
///
/// # Returns
/// Composant URLs with the model query added, in page order.
fn composant_endpoints(page: &Value) -> Vec<String> {
    let Some(sections) = page.pointer("/sections").and_then(Value::as_array) else {
        return Vec::new();
    };
    sections
        .iter()
        .filter_map(|section| section.get("endpoint").and_then(Value::as_str))
        .map(str::trim)
        .filter(|endpoint| endpoint.contains("/composant/"))
        .map(|endpoint| {
            let absolute = if endpoint.starts_with("http") {
                endpoint.to_string()
            } else {
                format!("{BFF_BASE_URL}{endpoint}")
            };
            with_model_query(&absolute)
        })
        .collect()
}

/// Adds the BFF model query parameter when it is absent.
///
/// # Arguments
/// * `url` - Endpoint URL.
///
/// # Returns
/// The URL with `model=BFF_MODEL` appended when needed.
fn with_model_query(url: &str) -> String {
    if url.contains("model=") {
        return url.to_string();
    }
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}model={BFF_MODEL}")
}
