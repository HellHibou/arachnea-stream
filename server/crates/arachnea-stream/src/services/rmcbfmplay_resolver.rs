use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use arachnea_core::persistence::CredentialsStore;
use arachnea_proxy::http::proxy_service::proxied_url_with_countries;
use arachnea_scrapyfy::{HttpClient, ScraperAgregator, ScraperQueryCollectionParameter};

use crate::services::player_resolver::{
    normalize_stream_kind, proxy_drm_today_license_request, save_drm_today_license_proxy_url,
    PlayerResolverEndpoints, PlayerStreamResolver, ProxiedStreamResponse, ResolvedPlayerStream,
};

use crate::PLAYER_DEFAULT_USER_AGENT as USER_AGENT;
use crate::PLAYER_ERROR_BODY_PREVIEW_LENGTH;

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

/// Process-wide cache of authenticated RMC/BFM Play sessions.
static SESSION_CACHE: OnceLock<Mutex<HashMap<String, CachedSession>>> = OnceLock::new();
/// Serializes cache misses during the RMC/BFM Play login flow.
static LOGIN_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

/// Cognito tokens returned by the Connect gateway.
#[derive(Clone)]
struct CognitoSession {
    id_token: String,
    access_token: String,
    #[allow(dead_code)]
    refresh_token: String,
}

/// Cached Cognito session with its expiry instant.
struct CachedSession {
    session: CognitoSession,
    expires_at: Instant,
}

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

/// Authenticated playback parts shared by replay and live.
struct PlaybackParts {
    manifest_url: String,
    manifest_type: String,
    license: Option<(String, String)>,
    storyboard_vtt_url: Option<String>,
}

/// Builds the public player stream from authenticated playback parts.
///
/// # Arguments
/// * `parts` - Manifest, optional license and storyboard data.
/// * `proxy_countries` - Countries selected for outbound routing.
/// * `endpoints` - Public endpoint settings used to build proxy URLs.
///
/// # Returns
/// The resolved stream with proxied manifest, license and storyboard.
fn resolved_stream(
    parts: PlaybackParts,
    proxy_countries: &[String],
    endpoints: &PlayerResolverEndpoints,
) -> Result<ResolvedPlayerStream> {
    let manifest_url = parts.manifest_url;
    let manifest_type = parts.manifest_type;
    // The Widevine license goes through the shared DRM Today proxy (same as
    // M6 Play): `x-dt-auth-token` challenge forwarding with wrapped-license
    // normalization, so the player receives a directly usable license.
    // Clear manifests carry no license pair.
    let license_url = parts.license.map(|(license_url, play_token)| {
        save_drm_today_license_proxy_url(
            endpoints,
            SERVICE_ID,
            &play_token,
            &normalize_stream_kind(None),
            &license_url,
            None,
        )
    });
    let storyboard_vtt_url = parts.storyboard_vtt_url.map(|url| {
        proxied_url_with_countries(
            &url,
            endpoints.http_proxy_public_path.as_deref(),
            proxy_countries,
            &[],
            &[],
        )
    });
    Ok(ResolvedPlayerStream {
        stream_url: vec![proxied_url_with_countries(
            &manifest_url,
            endpoints.http_proxy_public_path.as_deref(),
            proxy_countries,
            &[],
            &[],
        )],
        manifest_type: Some(manifest_type),
        license_url,
        license_headers: HashMap::new(),
        storyboard_vtt_url,
        ..Default::default()
    })
}

/// Returns a cached session or signs in through the Connect gateway.
///
/// # Arguments
/// * `http_client` - HTTP client used for the sign-in request.
/// * `credentials_store` - Store providing the RMC/BFM Play credentials.
///
/// # Errors
/// Returns an error when credentials are missing or blank, or sign-in fails.
///
/// # Returns
/// The authenticated Cognito session.
async fn get_or_login_session(
    http_client: &HttpClient,
    credentials_store: &dyn CredentialsStore,
) -> Result<CognitoSession> {
    let credentials = credentials_store
        .get_credentials(SERVICE_ID)
        .context("Failed to load RMC BFM Play credentials.")?
        .context("Missing credentials for service `rmcbfmplay-fr`.")?;
    let login = credentials.login.trim();
    let password = credentials.password.trim();
    if login.is_empty() || password.is_empty() {
        bail!("RMC BFM Play credentials must include a login and password.");
    }
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

    let session = sign_in(http_client, login, password).await?;
    save_cached_session(login, &session);
    Ok(session)
}

/// Signs in through the Connect gateway, mirroring the Connect login page
/// (`sign_in` endpoint, email lowercased/trimmed, password trimmed).
///
/// # Arguments
/// * `http_client` - HTTP client used for the gateway request.
/// * `login` - Account email.
/// * `password` - Account password.
///
/// # Errors
/// Returns an error when the gateway rejects the credentials or omits required
/// Cognito tokens.
///
/// # Returns
/// The authenticated Cognito session.
async fn sign_in(http_client: &HttpClient, login: &str, password: &str) -> Result<CognitoSession> {
    let body = json!({
        "endpoint": "sign_in",
        "payload": {
            "email": login.trim().to_lowercase(),
            "password": password.trim(),
            "clientId": CONNECT_CLIENT_ID,
        },
        "withRawResponse": true,
    })
    .to_string();
    let response = http_client
        .send_for_request(
            http::Method::POST,
            GATEWAY_URL,
            &gateway_headers(),
            Some(&body),
        )
        .await
        .context("Failed to authenticate with RMC BFM Play.")?;
    let status = response.status();
    let text = response
        .text()
        .await
        .context("Failed to authenticate with RMC BFM Play.")?;
    if !status.is_success() {
        let (kind, message) = parse_gateway_error(&text);
        bail!("RMC BFM Play rejected the configured credentials ({kind}): {message}");
    }
    let payload: Value = serde_json::from_str(&text).with_context(|| {
        format!(
            "Failed to authenticate with RMC BFM Play (HTTP {status}): invalid JSON: {}",
            preview_error_body(&text)
        )
    })?;
    let result = payload.pointer("/AuthenticationResult").with_context(|| {
        format!(
            "RMC BFM Play sign-in did not return authentication tokens ({})",
            describe_value(&payload)
        )
    })?;
    Ok(CognitoSession {
        id_token: read_string(result, "/IdToken", "Missing RMC BFM Play ID token.")?,
        access_token: read_string(result, "/AccessToken", "Missing RMC BFM Play access token.")?,
        refresh_token: read_string(
            result,
            "/RefreshToken",
            "Missing RMC BFM Play refresh token.",
        )?,
    })
}

/// Fetches a BFF document, trying the known Cognito token transports in order.
///
/// The exact transport the `/api/bff` server proxy injects is not documented;
/// each attempt is reported, and the successful variant is logged (never the
/// token), so the chain can be locked to the working variant once observed.
///
/// # Arguments
/// * `http_client` - HTTP client used for the request.
/// * `session` - Authenticated Cognito session providing candidate tokens.
/// * `url` - BFF document URL.
/// * `require_video` - Whether a successful response must contain `video`.
///
/// # Errors
/// Returns an error when every transport fails or lacks the required video.
///
/// # Returns
/// The first successful decoded BFF payload.
async fn fetch_bff_json(
    http_client: &HttpClient,
    session: &CognitoSession,
    url: &str,
    require_video: bool,
) -> Result<Value> {
    let variants: [(&str, String); 4] = [
        ("bearer-access", format!("Bearer {}", session.access_token)),
        ("bearer-id", format!("Bearer {}", session.id_token)),
        ("raw-access", session.access_token.clone()),
        ("raw-id", session.id_token.clone()),
    ];
    let mut attempts = Vec::new();
    for (name, authorization) in &variants {
        let mut headers = playback_headers();
        headers.insert("authorization".to_string(), authorization.clone());
        let response = match http_client
            .send_for_request(http::Method::GET, url, &headers, None)
            .await
        {
            Ok(response) => response,
            Err(error) => {
                attempts.push(format!("{name}: transport {error:#}"));
                continue;
            }
        };
        let status = response.status();
        let text = match response.text().await {
            Ok(text) => text,
            Err(error) => {
                attempts.push(format!("{name}: unreadable body {error:#}"));
                continue;
            }
        };
        if !status.is_success() {
            attempts.push(format!(
                "{name}: HTTP {status}: {}",
                preview_error_body(&text)
            ));
            continue;
        }
        let payload: Value = match serde_json::from_str(&text) {
            Ok(payload) => payload,
            Err(_) => {
                attempts.push(format!(
                    "{name}: HTTP {status} invalid JSON: {}",
                    preview_error_body(&text)
                ));
                continue;
            }
        };
        if require_video
            && payload
                .pointer("/video")
                .filter(|video| video.is_object())
                .is_none()
        {
            attempts.push(format!("{name}: HTTP {status} without video payload"));
            continue;
        }
        tracing::info!(
            variant = name,
            url = url,
            "RMC BFM Play authenticated BFF request succeeded"
        );
        return Ok(payload);
    }
    bail!(
        "RMC BFM Play BFF request failed for `{url}`: {}",
        attempts.join("; ")
    )
}

/// Extracts playback from an authenticated `video` payload.
///
/// The manifest is always required. The license pair is required only when
/// the manifest itself is encrypted (probed directly); clear manifests
/// return a license-less stream instead of bailing. The storyboard VTT is
/// best-effort and never fails the stream.
///
/// # Arguments
/// * `http_client` - HTTP client used to probe the manifest.
/// * `video` - Authenticated `video` payload.
///
/// # Errors
/// Returns an error when no manifest URL exists or a required license is
/// missing for an encrypted manifest.
///
/// # Returns
/// The extracted playback parts.
async fn extract_playback(http_client: &HttpClient, video: &Value) -> Result<PlaybackParts> {
    extract_playback_parts(http_client, video)
        .await
        .with_context(|| {
            format!(
                "RMC BFM Play did not return playback URLs ({})",
                describe_value(video)
            )
        })
}

/// Extracts manifest, license and storyboard candidates from a video payload.
///
/// # Arguments
/// * `http_client` - HTTP client used to probe the manifest.
/// * `video` - Authenticated `video` payload.
///
/// # Errors
/// Returns an error when no manifest exists or an encrypted manifest lacks a
/// license URL or play token.
///
/// # Returns
/// The selected playback parts.
async fn extract_playback_parts(http_client: &HttpClient, video: &Value) -> Result<PlaybackParts> {
    // Collect every manifest candidate: explicit pointers first, then content
    // scans. DASH is preferred over HLS whenever both exist: the shared DRM
    // Today Widevine license path is validated for DASH, while Video.js/VHS
    // cannot drive a Widevine license for SAMPLE-AES HLS (FairPlay on Safari
    // and PlayReady on Edge only), which would load segments without ever
    // decoding them.
    let mut manifest_source = "pointer";
    let pointer_hits: Vec<String> = [
        "/url",
        "/manifest",
        "/manifest_url",
        "/src",
        "/stream_url",
        "/dash_url",
        "/file",
        "/stream/url",
        "/stream/manifest",
        "/stream/dash",
        "/stream/hls",
        "/stream/src",
        "/stream/file",
        "/hls",
    ]
    .iter()
    .filter_map(|pointer| read_opt_string(video, pointer))
    .collect();
    let mpd_urls = find_all_urls_containing(video, ".mpd");
    let m3u8_urls = find_all_urls_containing(video, ".m3u8");
    let manifest_url = pointer_hits
        .iter()
        .find(|url| url.contains(".mpd"))
        .or_else(|| {
            manifest_source = ".mpd scan";
            mpd_urls.first()
        })
        .or_else(|| {
            manifest_source = "pointer-hls";
            pointer_hits.iter().find(|url| url.contains(".m3u8"))
        })
        .or_else(|| {
            manifest_source = ".m3u8 scan";
            m3u8_urls.first()
        })
        .or_else(|| {
            manifest_source = "pointer-other";
            pointer_hits.first()
        })
        .cloned();
    let Some(manifest_url) = manifest_url else {
        bail!(
            "RMC BFM Play video has no manifest URL ({})",
            describe_value(video)
        )
    };
    let manifest_type = manifest_type_from_url(&manifest_url);
    tracing::info!(
        manifest_source,
        manifest_type,
        mpd_candidates = mpd_urls.len(),
        m3u8_candidates = m3u8_urls.len(),
        "RMC BFM Play manifest candidates"
    );
    let mut license_source = "pointer";
    let license_url = [
        "/drm/widevine/url",
        "/drm/widevine/license",
        "/drm/widevine/license_url",
        "/drm/license_url",
        "/drm/license",
        "/drm/licence",
        "/license_url",
        "/license",
        "/licence",
    ]
    .iter()
    .find_map(|pointer| read_opt_string(video, pointer))
    .or_else(|| {
        // The Widevine license object may use other field names; prefer any
        // URL from its subtree before falling back to other DRM systems.
        license_source = "widevine subtree";
        find_http_string(video.pointer("/drm/widevine").unwrap_or(&Value::Null))
    })
    .or_else(|| {
        license_source = "widevine-url scan";
        find_url_containing(video, "widevine")
    })
    .or_else(|| {
        license_source = "licen-key scan";
        find_keyed_url(video, "licen")
    });
    let Some(license_url) = license_url else {
        // No license material found: check the manifest itself. Clear
        // manifests play without a license; only encrypted ones bail.
        if !manifest_requires_drm(http_client, &manifest_url, &manifest_type).await {
            tracing::info!(
                manifest_source,
                manifest_type,
                video_shape = %describe_value(video),
                "RMC BFM Play clear manifest, no license required"
            );
            return Ok(PlaybackParts {
                manifest_url,
                manifest_type,
                license: None,
                storyboard_vtt_url: extract_storyboard_vtt(video),
            });
        }
        bail!(
            "RMC BFM Play video has no license URL ({})",
            describe_value(video)
        )
    };
    let play_token = ["/drm/play_token", "/play_token", "/drm/token", "/token"]
        .iter()
        .find_map(|pointer| read_opt_string(video, pointer));
    let Some(play_token) = play_token else {
        bail!(
            "RMC BFM Play video has no play token ({})",
            describe_value(video)
        )
    };
    tracing::info!(
        manifest_source,
        manifest_type,
        license_source,
        play_token_found = true,
        license_headers = "x-dt-auth-token",
        storyboard_vtt_found = %extract_storyboard_vtt(video).is_some(),
        video_shape = %describe_value(video),
        "RMC BFM Play playback mapping selected"
    );
    Ok(PlaybackParts {
        manifest_url,
        manifest_type,
        license: Some((license_url, play_token)),
        storyboard_vtt_url: extract_storyboard_vtt(video),
    })
}

/// Extracts the storyboard WebVTT URL from the BFF `sprites` field.
///
/// Best-effort only: returns `None` when no VTT turns up, never failing the
/// stream. Only the field layout is logged, never payload values.
///
/// # Arguments
/// * `video` - Authenticated `video` payload.
///
/// # Returns
/// The storyboard VTT URL, or `None` when absent.
fn extract_storyboard_vtt(video: &Value) -> Option<String> {
    let sprites = video.pointer("/sprites").unwrap_or(&Value::Null);
    if let Value::Null = sprites {
        return None;
    }
    if let Some(shape) = sprites.as_object() {
        let keys: Vec<_> = shape.keys().map(String::as_str).collect();
        tracing::info!(sprites_keys = %keys.join(","), "RMC BFM Play sprites field");
    }
    ["/url", "/vtt", "/src", "/file"]
        .iter()
        .find_map(|pointer| read_opt_string(sprites, pointer))
        .filter(|url| url.to_ascii_lowercase().contains(".vtt"))
        .or_else(|| find_all_urls_containing(sprites, ".vtt").into_iter().next())
}

/// Probes a manifest for encryption markers (`EXT-X-KEY` for HLS,
/// `ContentProtection` for DASH). Unreadable manifests conservatively count
/// as encrypted so negotiation keeps requiring a license.
///
/// # Arguments
/// * `http_client` - HTTP client used to fetch the manifest.
/// * `manifest_url` - Manifest URL to probe.
/// * `manifest_type` - Manifest type, `m3u8` or `mpd`.
///
/// # Returns
/// `true` when the manifest is encrypted or cannot be read.
async fn manifest_requires_drm(
    http_client: &HttpClient,
    manifest_url: &str,
    manifest_type: &str,
) -> bool {
    let text = match http_client
        .query_http_for_request(http::Method::GET, manifest_url, &playback_headers(), None)
        .await
    {
        Ok(text) => text,
        Err(_) => return true,
    };
    if manifest_type == "m3u8" {
        text.contains("EXT-X-KEY")
    } else {
        text.contains("ContentProtection")
    }
}

/// Determines the manifest type from its URL.
///
/// # Arguments
/// * `url` - Manifest URL.
///
/// # Returns
/// `m3u8` for URLs containing that extension, `mpd` otherwise.
fn manifest_type_from_url(url: &str) -> String {
    if url.to_ascii_lowercase().contains(".m3u8") {
        return "m3u8".to_string();
    }

    "mpd".to_string()
}

/// Builds the headers used for Connect gateway requests.
///
/// # Returns
/// JSON request and acceptance headers with the resolver user-agent.
fn gateway_headers() -> HashMap<String, String> {
    HashMap::from([
        ("user-agent".to_string(), USER_AGENT.to_string()),
        ("content-type".to_string(), "application/json".to_string()),
        (
            "accept".to_string(),
            "application/json, text/plain, */*".to_string(),
        ),
    ])
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

/// Reads the Cognito error kind and message without exposing credentials.
///
/// # Arguments
/// * `body` - Gateway error response body.
///
/// # Returns
/// The normalized error kind and a safe message.
fn parse_gateway_error(body: &str) -> (String, String) {
    if let Ok(payload) = serde_json::from_str::<Value>(body) {
        let kind = payload
            .get("__type")
            .or_else(|| payload.get("errorCode"))
            .or_else(|| payload.get("error"))
            .and_then(Value::as_str)
            .map(|kind| kind.rsplit('#').next().unwrap_or(kind).trim().to_string())
            .filter(|kind| !kind.is_empty())
            .unwrap_or_else(|| "UnknownError".to_string());
        let message = payload
            .get("message")
            .or_else(|| payload.get("errorMessage"))
            .or_else(|| payload.get("error"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|message| !message.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| preview_error_body(body));
        return (kind, message);
    }
    ("UnknownError".to_string(), preview_error_body(body))
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

/// Summarizes the section types present in a BFF page.
///
/// # Arguments
/// * `page` - Decoded BFF page payload.
///
/// # Returns
/// A diagnostic string listing section types.
fn describe_sections(page: &Value) -> String {
    let types = page
        .pointer("/sections")
        .and_then(Value::as_array)
        .map(|sections| {
            sections
                .iter()
                .filter_map(|section| section.get("type").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    format!("section types=[{types}]")
}

/// Describes an unexpected payload shape for diagnostics: object keys and
/// URLs with query strings redacted, never token values.
///
/// # Arguments
/// * `payload` - Payload to describe.
///
/// # Returns
/// A bounded diagnostic string with keys and redacted URLs.
fn describe_value(payload: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(object) = payload.as_object() {
        let keys: Vec<_> = object.keys().map(String::as_str).collect();
        parts.push(format!("keys=[{}]", keys.join(",")));
    }
    if let Some(drm) = payload.pointer("/drm").and_then(Value::as_object) {
        let keys: Vec<_> = drm.keys().map(String::as_str).collect();
        parts.push(format!("drm.keys=[{}]", keys.join(",")));
        for system in ["widevine", "fairplay", "playready"] {
            if let Some(child) = drm.get(system).and_then(Value::as_object) {
                let keys: Vec<_> = child.keys().map(String::as_str).collect();
                parts.push(format!("drm.{system}.keys=[{}]", keys.join(",")));
            }
        }
    }
    if let Some(stream) = payload.pointer("/stream").and_then(Value::as_object) {
        let keys: Vec<_> = stream.keys().map(String::as_str).collect();
        parts.push(format!("stream.keys=[{}]", keys.join(",")));
    }
    let mut urls = Vec::new();
    collect_strings(payload, &mut urls, 20);
    let redacted: Vec<String> = urls
        .into_iter()
        .filter(|value| value.starts_with("http"))
        .map(|url| redact_url(&url))
        .collect();
    if !redacted.is_empty() {
        parts.push(format!("urls=[{}]", redacted.join(",")));
    }
    if parts.is_empty() {
        parts.push("non-object payload".to_string());
    }
    parts.join(" ")
}

/// Recursively collects string values up to a limit.
///
/// # Arguments
/// * `payload` - JSON value to traverse.
/// * `output` - Accumulator receiving strings.
/// * `limit` - Maximum number of strings to collect.
fn collect_strings(payload: &Value, output: &mut Vec<String>, limit: usize) {
    if output.len() >= limit {
        return;
    }
    match payload {
        Value::String(value) => output.push(value.clone()),
        Value::Array(items) => {
            for item in items {
                collect_strings(item, output, limit);
            }
        }
        Value::Object(entries) => {
            for value in entries.values() {
                collect_strings(value, output, limit);
            }
        }
        _ => {}
    }
}

/// Hides the query string of a URL for diagnostics.
///
/// # Arguments
/// * `url` - URL to redact.
///
/// # Returns
/// The base URL with `?<redacted>` when a query was present.
fn redact_url(url: &str) -> String {
    match url.split_once('?') {
        Some((base, _)) => format!("{base}?<redacted>"),
        None => url.to_string(),
    }
}

/// Reads a trimmed non-empty string at a JSON pointer.
///
/// # Arguments
/// * `payload` - JSON value to query.
/// * `pointer` - JSON pointer selecting the string.
///
/// # Returns
/// The trimmed string, or `None` when absent, blank or not a string.
fn read_opt_string(payload: &Value, pointer: &str) -> Option<String> {
    payload
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Returns the first HTTP URL containing a marker.
///
/// # Arguments
/// * `payload` - JSON value to search.
/// * `marker` - Required URL substring.
///
/// # Returns
/// The first matching URL, or `None` when none exists.
fn find_url_containing(payload: &Value, marker: &str) -> Option<String> {
    find_all_urls_containing(payload, marker).into_iter().next()
}

/// Collects HTTP URLs containing a marker.
///
/// # Arguments
/// * `payload` - JSON value to search.
/// * `marker` - Required URL substring.
///
/// # Returns
/// Matching URLs in traversal order, bounded by the collection limit.
fn find_all_urls_containing(payload: &Value, marker: &str) -> Vec<String> {
    let mut all = Vec::new();
    collect_strings(payload, &mut all, 50);
    all.into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| value.starts_with("http") && value.contains(marker))
        .collect()
}

/// Returns the first HTTP string found in a payload.
///
/// # Arguments
/// * `payload` - JSON value to search.
///
/// # Returns
/// The first trimmed string starting with `http`, or `None`.
fn find_http_string(payload: &Value) -> Option<String> {
    let mut found = None;
    find_string(payload, &mut found, &|value| value.starts_with("http"));
    found
}

/// Finds an HTTP URL stored under a key containing a marker.
///
/// # Arguments
/// * `payload` - JSON value to search.
/// * `key_marker` - Case-insensitive key substring.
///
/// # Returns
/// The first matching HTTP URL, or `None`.
fn find_keyed_url(payload: &Value, key_marker: &str) -> Option<String> {
    match payload {
        Value::Object(object) => {
            for (key, value) in object {
                if key.to_lowercase().contains(key_marker) {
                    if let Some(url) = value
                        .as_str()
                        .map(str::trim)
                        .filter(|value| value.starts_with("http"))
                    {
                        return Some(url.to_string());
                    }
                }
                if let Some(url) = find_keyed_url(value, key_marker) {
                    return Some(url);
                }
            }
            None
        }
        Value::Array(items) => {
            for item in items {
                if let Some(url) = find_keyed_url(item, key_marker) {
                    return Some(url);
                }
            }
            None
        }
        _ => None,
    }
}

/// Finds the first string matching a predicate.
///
/// # Arguments
/// * `payload` - JSON value to search.
/// * `found` - Accumulator receiving the first match.
/// * `matches` - Predicate applied to trimmed string values.
fn find_string(payload: &Value, found: &mut Option<String>, matches: &dyn Fn(&str) -> bool) {
    if found.is_some() {
        return;
    }
    match payload {
        Value::String(value) if matches(value.trim()) => {
            *found = Some(value.trim().to_string());
        }
        Value::Array(items) => {
            for item in items {
                find_string(item, found, matches);
            }
        }
        Value::Object(entries) => {
            for value in entries.values() {
                find_string(value, found, matches);
            }
        }
        _ => {}
    }
}

/// Reads a non-empty string at a JSON pointer.
///
/// # Arguments
/// * `payload` - JSON value to query.
/// * `pointer` - JSON pointer selecting the string.
/// * `error` - Error message used when the value is absent or blank.
///
/// # Errors
/// Returns an error when the pointer does not resolve to a non-empty string.
///
/// # Returns
/// The trimmed string value.
fn read_string(payload: &Value, pointer: &str, error: &str) -> Result<String> {
    payload
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .context(error.to_string())
}

/// Produces a bounded preview of an HTTP error body.
///
/// # Arguments
/// * `body` - Raw response body.
///
/// # Returns
/// A trimmed preview capped by `PLAYER_ERROR_BODY_PREVIEW_LENGTH`, with an
/// ellipsis when longer, or `<empty body>` when blank.
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

/// Caches a session for `login` using `SESSION_TTL`.
///
/// # Arguments
/// * `login` - Account email used as the cache key.
/// * `session` - Session to store.
///
/// The write is skipped when the cache mutex is poisoned.
fn save_cached_session(login: &str, session: &CognitoSession) {
    let cache = SESSION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut cache) = cache.lock() {
        cache.retain(|_, entry| entry.expires_at > Instant::now());
        cache.insert(
            login.to_string(),
            CachedSession {
                session: session.clone(),
                expires_at: Instant::now() + SESSION_TTL,
            },
        );
    }
}

/// Loads a non-expired session, pruning expired entries.
///
/// # Arguments
/// * `login` - Account email used as the cache key.
///
/// # Returns
/// The cached session, or `None` when absent, expired, or the cache mutex is
/// poisoned.
fn load_cached_session(login: &str) -> Option<CognitoSession> {
    let cache = SESSION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache) = cache.lock() else {
        return None;
    };
    cache.retain(|_, entry| entry.expires_at > Instant::now());
    cache.get(login).map(|entry| entry.session.clone())
}
