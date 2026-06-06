use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use arachnea_core::persistence::CredentialsStore;
use arachnea_scrapyfy::{HttpClient, ScraperQueryCollectionParameter};

use crate::services::player_resolver::{
    normalize_stream_kind, proxy_drm_today_license_request, save_drm_today_license_proxy_url,
    PlayerStreamResolver, ProxiedStreamResponse, ResolvedPlayerStream,
};

const SIXPLAY_BASE_URL: &str = "https://www.6play.fr";
const SIXPLAY_LOGIN_URL: &str = "https://login-gigya.m6.fr/accounts.login";
const SIXPLAY_LOGIN_PAGE_URL: &str = "https://www.6play.fr/connexion";
const SIXPLAY_BUNDLE_URL_TEMPLATE: &str = "https://www.6play.fr/main-{}.bundle.js";
const SIXPLAY_API_KEY_FALLBACK: &str =
    "3_hH5KBv25qZTd_sURpixbQW6a4OsiIzIEF2Ei_2H7TXTGLJb_1Hr4THKZianCQhWK";
const SIXPLAY_TOKEN_UUID_URL: &str = "https://front-auth.6cloud.fr/v2/platforms/m6group_web/getJwt";
const SIXPLAY_TOKEN_REPLAY_URL_TEMPLATE: &str = "https://drm.6cloud.fr/v1/customers/m6web/platforms/m6group_web/services/m6replay/users/{}/videos/{}/upfront-token";
const SIXPLAY_TOKEN_LIVE_URL_TEMPLATE: &str = "https://drm.6cloud.fr/v1/customers/m6web/platforms/m6group_web/services/6play/users/{}/live/{}/upfront-token";
const SIXPLAY_VIDEO_JSON_URL_TEMPLATE: &str = "https://android.middleware.6play.fr/6play/v2/platforms/m6group_androidmob/services/6play/videos/{}?csa=6&with=clips,freemiumpacks";
const SIXPLAY_LIVE_JSON_URL: &str =
    "https://android.middleware.6play.fr/6play/v2/platforms/m6group_androidmob/services/6play/live";
const SIXPLAY_LICENSE_URL: &str =
    "https://lic.drmtoday.com/license-proxy-widevine/cenc/?specConform=true";
const SIXPLAY_CLIENT_RELEASE: &str = "5.103.3";
const SIXPLAY_CUSTOMER_NAME: &str = "m6web";
const SIXPLAY_CALLBACK_NAME: &str = "jsonp_arachnea";
const SIXPLAY_DEVICE_ID: &str = "_luid_arachnea";
const SIXPLAY_SESSION_TTL: Duration = Duration::from_secs(15 * 60);
const M6PLAY_SERVICE_ID: &str = "m6play-fr";

static SIXPLAY_JS_ID_REGEX: OnceLock<Regex> = OnceLock::new();
static SIXPLAY_API_KEY_REGEX: OnceLock<Regex> = OnceLock::new();
static SIXPLAY_SESSION_CACHE: OnceLock<Mutex<HashMap<String, CachedSixPlaySession>>> =
    OnceLock::new();

#[derive(Clone)]
struct SixPlaySession {
    account_id: String,
    login_token: String,
}

struct CachedSixPlaySession {
    session: SixPlaySession,
    expires_at: Instant,
}

/// M6+ implementation of the generic protected playback resolver contract.
pub(crate) struct M6PlayResolver;

#[async_trait]
impl PlayerStreamResolver for M6PlayResolver {
    fn source_id(&self) -> &'static str {
        M6PLAY_SERVICE_ID
    }

    async fn resolve_player_stream(
        &self,
        credentials_store: &dyn CredentialsStore,
        resolver_kind: &str,
        resolver_target: &str,
        resolver_stream_kind: Option<String>,
        _service_parameters: &[ScraperQueryCollectionParameter],
    ) -> Result<ResolvedPlayerStream> {
        match resolver_kind.trim() {
            "m6play-video" => {
                resolve_replay_stream(credentials_store, resolver_target, resolver_stream_kind)
                    .await
            }
            "m6play-live" => {
                resolve_live_stream(credentials_store, resolver_target, resolver_stream_kind).await
            }
            kind => bail!(
                "Unsupported player resolver `{}` for source `{}`.",
                kind,
                M6PLAY_SERVICE_ID
            ),
        }
    }

    async fn get_stream(&self, stream_token: &str, body: &[u8]) -> Result<ProxiedStreamResponse> {
        proxy_drm_today_license_request(stream_token, body).await
    }
}

async fn resolve_replay_stream(
    credentials_store: &dyn CredentialsStore,
    video_id: &str,
    stream_kind: Option<String>,
) -> Result<ResolvedPlayerStream> {
    let normalized_video_id = video_id.trim();
    if normalized_video_id.is_empty() {
        bail!("Missing 6play video identifier.");
    }

    let http_client = HttpClient::new(SIXPLAY_BASE_URL);
    let session = get_or_login_session(&http_client, credentials_store).await?;
    let upfront_token = fetch_upfront_token(&http_client, &session, normalized_video_id).await?;
    let manifest_url = fetch_best_manifest_url(&http_client, normalized_video_id).await?;
    let stream_kind = normalize_stream_kind(stream_kind);
    let license_url = save_drm_today_license_proxy_url(
        M6PLAY_SERVICE_ID,
        &upfront_token,
        &stream_kind,
        SIXPLAY_LICENSE_URL,
        None,
    );

    Ok(ResolvedPlayerStream {
        stream_url: manifest_url,
        manifest_type: "mpd".to_string(),
        license_url: Some(license_url),
        license_headers: HashMap::new(),
    })
}

async fn resolve_live_stream(
    credentials_store: &dyn CredentialsStore,
    channel_id: &str,
    stream_kind: Option<String>,
) -> Result<ResolvedPlayerStream> {
    let normalized_channel = channel_id.trim();
    if normalized_channel.is_empty() {
        bail!("Missing 6play channel identifier.");
    }

    let http_client = HttpClient::new(SIXPLAY_BASE_URL);
    let session = get_or_login_session(&http_client, credentials_store).await?;

    // Map channel identifier to the API's live_item_id format.
    let live_item_id = match normalized_channel {
        "6ter" => "6T".to_string(),
        "gulli" => "gulli".to_string(),
        other => other.to_uppercase(),
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
        .await?;

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
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:148.0) Gecko/20100101 Firefox/148.0"
            .to_string(),
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
        M6PLAY_SERVICE_ID,
        &token,
        &stream_kind,
        SIXPLAY_LICENSE_URL,
        None,
    );

    Ok(ResolvedPlayerStream {
        stream_url: manifest_url,
        manifest_type: "mpd".to_string(),
        license_url: Some(license_url),
        license_headers: HashMap::new(),
    })
}

async fn get_or_login_session(
    http_client: &HttpClient,
    credentials_store: &dyn CredentialsStore,
) -> Result<SixPlaySession> {
    let (login, password) = load_credentials(credentials_store)?;

    if let Some(session) = load_cached_session(&login) {
        return Ok(session);
    }

    let api_key = fetch_api_key(http_client)
        .await
        .unwrap_or_else(|_| SIXPLAY_API_KEY_FALLBACK.to_string());

    let payload = serde_urlencoded::to_string([
        ("loginID", login.as_str()),
        ("password", password.as_str()),
        ("apiKey", api_key.as_str()),
        ("format", "jsonp"),
        ("callback", SIXPLAY_CALLBACK_NAME),
    ])
    .context("Failed to serialize the 6play login payload.")?;

    let mut login_headers = HashMap::new();
    login_headers.insert(
        "content-type".to_string(),
        "application/x-www-form-urlencoded".to_string(),
    );
    login_headers.insert("referer".to_string(), SIXPLAY_LOGIN_PAGE_URL.to_string());

    let login_response = http_client
        .query_http_for_request(
            http::Method::POST,
            SIXPLAY_LOGIN_URL,
            &login_headers,
            Some(&payload),
        )
        .await?;

    let login_payload = parse_jsonp_payload(&login_response)?;
    let account_id = read_json_string(
        &login_payload,
        &["UID"],
        "Missing 6play account identifier in login response.",
    )?;
    let account_signature = read_json_string(
        &login_payload,
        &["UIDSignature"],
        "Missing 6play account signature in login response.",
    )?;
    let account_timestamp = read_json_string(
        &login_payload,
        &["signatureTimestamp"],
        "Missing 6play signature timestamp in login response.",
    )?;

    let mut uuid_headers = HashMap::new();
    uuid_headers.insert(
        "x-auth-gigya-signature".to_string(),
        account_signature.clone(),
    );
    uuid_headers.insert(
        "x-auth-gigya-signature-timestamp".to_string(),
        account_timestamp,
    );
    uuid_headers.insert("x-auth-gigya-uid".to_string(), account_id.clone());
    uuid_headers.insert(
        "x-auth-device-id".to_string(),
        SIXPLAY_DEVICE_ID.to_string(),
    );
    uuid_headers.insert(
        "x-customer-name".to_string(),
        SIXPLAY_CUSTOMER_NAME.to_string(),
    );

    let uuid_payload = http_client
        .get_json_for_request(
            http::Method::GET,
            SIXPLAY_TOKEN_UUID_URL,
            &uuid_headers,
            None,
        )
        .await?;
    let login_token = read_json_string(
        &uuid_payload,
        &["token"],
        "Missing 6play login token in front-auth response.",
    )?;

    let session = SixPlaySession {
        account_id,
        login_token,
    };

    save_cached_session(&login, &session);
    Ok(session)
}

async fn fetch_api_key(http_client: &HttpClient) -> Result<String> {
    let login_page = http_client
        .query_http(http::Method::GET, SIXPLAY_LOGIN_PAGE_URL)
        .await?;
    let js_id_regex = SIXPLAY_JS_ID_REGEX.get_or_init(|| {
        Regex::new(r"main-(.*?)\.bundle\.js").expect("Invalid 6play JS identifier regex.")
    });
    let Some(js_id) = js_id_regex
        .captures(&login_page)
        .and_then(|captures| captures.get(1))
        .map(|capture| capture.as_str().to_string())
    else {
        bail!("Missing 6play bundle identifier.");
    };

    let bundle_url = SIXPLAY_BUNDLE_URL_TEMPLATE.replace("{}", &js_id);
    let bundle = http_client
        .query_http(http::Method::GET, &bundle_url)
        .await?;
    let api_key_regex = SIXPLAY_API_KEY_REGEX.get_or_init(|| {
        Regex::new(r#""eu1\.gigya\.com",key:"(.*?)""#).expect("Invalid 6play API key regex.")
    });

    api_key_regex
        .captures(&bundle)
        .and_then(|captures| captures.get(1))
        .map(|capture| capture.as_str().to_string())
        .ok_or_else(|| anyhow!("Missing 6play API key in web bundle."))
}

async fn fetch_upfront_token(
    http_client: &HttpClient,
    session: &SixPlaySession,
    video_id: &str,
) -> Result<String> {
    let token_url = SIXPLAY_TOKEN_REPLAY_URL_TEMPLATE
        .replacen("{}", &session.account_id, 1)
        .replacen("{}", video_id, 1);

    let mut headers = HashMap::new();
    headers.insert(
        "x-customer-name".to_string(),
        SIXPLAY_CUSTOMER_NAME.to_string(),
    );
    headers.insert(
        "x-client-release".to_string(),
        SIXPLAY_CLIENT_RELEASE.to_string(),
    );
    headers.insert(
        "authorization".to_string(),
        format!("Bearer {}", session.login_token),
    );

    let token_payload = http_client
        .get_json_for_request(http::Method::GET, &token_url, &headers, None)
        .await?;

    read_json_string(
        &token_payload,
        &["token"],
        "Missing 6play upfront token for replay playback.",
    )
}

async fn fetch_best_manifest_url(http_client: &HttpClient, video_id: &str) -> Result<String> {
    let video_url = SIXPLAY_VIDEO_JSON_URL_TEMPLATE.replace("{}", video_id);
    let mut headers = HashMap::new();
    headers.insert(
        "x-customer-name".to_string(),
        SIXPLAY_CUSTOMER_NAME.to_string(),
    );

    let video_payload = http_client
        .get_json_for_request(http::Method::GET, &video_url, &headers, None)
        .await?;
    let assets = video_payload
        .pointer("/clips/0/assets")
        .and_then(Value::as_array)
        .context("Missing 6play replay assets in video payload.")?;

    let manifest_url = select_best_asset_url(assets, "usp_dashcenc_h264")
        .context("Missing 6play DASH DRM manifest for this replay asset.")?;

    http_client
        .resolve_final_url_for_request(http::Method::GET, &manifest_url, &HashMap::new(), None)
        .await
        .or(Ok(manifest_url))
}

fn select_best_asset_url(assets: &[Value], asset_type: &str) -> Option<String> {
    assets
        .iter()
        .filter(|asset| asset.get("type").and_then(Value::as_str) == Some(asset_type))
        .filter_map(|asset| {
            let url = asset.get("full_physical_path").and_then(Value::as_str)?;
            let quality = asset
                .get("video_quality")
                .and_then(Value::as_str)
                .map(quality_rank)
                .unwrap_or_default();

            Some((quality, url.to_string()))
        })
        .max_by_key(|(quality, _)| *quality)
        .map(|(_, url)| url)
}

fn quality_rank(value: &str) -> usize {
    match value.trim().to_ascii_lowercase().as_str() {
        "uhd" => 4,
        "fhd" => 3,
        "hd" => 2,
        "sd" => 1,
        _ => 0,
    }
}

fn parse_jsonp_payload(payload: &str) -> Result<Value> {
    let trimmed = payload.trim();
    let Some(open_paren_index) = trimmed.find('(') else {
        bail!("Invalid 6play JSONP login payload.");
    };
    let Some(close_paren_index) = trimmed.rfind(')') else {
        bail!("Invalid 6play JSONP login payload.");
    };

    let json_payload = trimmed[open_paren_index + 1..close_paren_index].trim();
    serde_json::from_str(json_payload).context("Invalid JSON inside the 6play JSONP response.")
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

fn load_credentials(credentials_store: &dyn CredentialsStore) -> Result<(String, String)> {
    let credentials = credentials_store
        .get_credentials(M6PLAY_SERVICE_ID)
        .with_context(|| {
            format!(
                "Failed to load credentials for service `{}` from the configured store.",
                M6PLAY_SERVICE_ID
            )
        })?
        .with_context(|| {
            format!(
                "Missing credentials for service `{}` in the configured store.",
                M6PLAY_SERVICE_ID
            )
        })?;

    Ok((credentials.login, credentials.password))
}

fn load_cached_session(login: &str) -> Option<SixPlaySession> {
    let cache = SIXPLAY_SESSION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache_guard) = cache.lock() else {
        return None;
    };

    let now = Instant::now();
    cache_guard.retain(|_, entry| entry.expires_at > now);
    cache_guard.get(login).map(|entry| entry.session.clone())
}

fn save_cached_session(login: &str, session: &SixPlaySession) {
    let cache = SIXPLAY_SESSION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache_guard) = cache.lock() else {
        return;
    };

    cache_guard.insert(
        login.to_string(),
        CachedSixPlaySession {
            session: session.clone(),
            expires_at: Instant::now() + SIXPLAY_SESSION_TTL,
        },
    );
}
