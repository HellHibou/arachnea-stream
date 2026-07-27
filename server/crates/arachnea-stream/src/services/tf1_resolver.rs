use anyhow::{bail, Context, Result};
use arachnea_proxy::http::{actions::ReplaceAll, proxy_service::proxied_url};
use async_trait::async_trait;
use rand::{distr::Alphanumeric, Rng};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use arachnea_core::persistence::CredentialsStore;
use arachnea_scrapyfy::{
    ArachneaHttpError, HttpClient, ScraperAgregator, ScraperHttpConfig,
    ScraperQueryCollectionParameter,
};

use crate::services::player_resolver::{
    normalize_stream_kind, Chapter, PlayerResolverEndpoints, PlayerStreamResolver,
    ProxiedStreamResponse, ResolvedPlayerImageTitle, ResolvedPlayerStream,
};

const TF1_SERVICE_ID: &str = "tf1-fr";
const TF1_BASE_URL: &str = "https://www.tf1.fr";
const TF1_GIGYA_API_KEY: &str =
    "3_hWgJdARhz_7l1oOp3a8BDLoR9cuWZpUaKG4aqF7gum9_iK3uTZ2VlDBl8ANf8FVk";
const TF1_BOOTSTRAP_URL: &str = "https://compte.tf1.fr/accounts.webSdkBootstrap";
const TF1_LOGIN_URL: &str = "https://compte.tf1.fr/accounts.login";
const TF1_TOKEN_URL: &str = "https://www.tf1.fr/token/gigya/web";
const TF1_MEDIA_INFO_URL_TEMPLATE: &str = "https://mediainfo.tf1.fr/mediainfocombo/{}";
const TF1_FALLBACK_LICENSE_URL_TEMPLATE: &str = "https://drm-wide.tf1.fr/proxy?id={}";
const TF1_PROXY_STREAM_KIND: &str = "widevine-license-proxy";
const TF1_PROXY_COUNTRY: &str = "FR";
const TF1_SESSION_TTL: Duration = Duration::from_secs(15 * 60);
const TF1_LICENSE_TTL: Duration = Duration::from_secs(15 * 60);

static TF1_SESSION_CACHE: OnceLock<Mutex<HashMap<String, CachedTf1Session>>> = OnceLock::new();
static TF1_LICENSE_CACHE: OnceLock<Mutex<HashMap<String, CachedTf1License>>> = OnceLock::new();
static TF1_LOGIN_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

#[derive(Clone)]
struct Tf1Session {
    token: String,
}

struct CachedTf1Session {
    session: Tf1Session,
    expires_at: Instant,
}

#[derive(Clone)]
struct CachedTf1License {
    license_url: String,
    headers: HashMap<String, String>,
    stream_kind: String,
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
        resolver: &str,
        target: &str,
        _service_parameters: &[ScraperQueryCollectionParameter],
        endpoints: &PlayerResolverEndpoints,
    ) -> Result<ResolvedPlayerStream> {
        match resolver.trim() {
            "tf1-video" => {
                resolve_replay_stream(
                    scraper_agregator,
                    credentials_store,
                    target,
                    None,
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

async fn resolve_replay_stream(
    scraper_agregator: &ScraperAgregator,
    credentials_store: &dyn CredentialsStore,
    video_id: &str,
    stream_kind: Option<String>,
    endpoints: &PlayerResolverEndpoints,
) -> Result<ResolvedPlayerStream> {
    let normalized_video_id = video_id.trim();
    if normalized_video_id.is_empty() {
        bail!("Missing TF1 video identifier.");
    }

    for attempt in 1..=3 {
        match resolve_replay_stream_with_config(
            scraper_agregator,
            credentials_store,
            normalized_video_id,
            stream_kind.clone(),
            tf1_http_config(),
            endpoints,
        )
        .await
        {
            Ok(stream) => return Ok(stream),
            Err(error) if is_proxy_error(&error) => {
                tracing::warn!(
                    attempt,
                    error = %error,
                    "TF1 FR proxy failed while resolving replay stream"
                );
                if attempt < 3 {
                    tokio::time::sleep(Duration::from_millis(250 * attempt)).await;
                    continue;
                }
            }
            Err(error) => return Err(error),
        }
    }

    tracing::warn!(
        "TF1 FR proxy retries exhausted while resolving replay stream; retrying without FR proxy"
    );
    resolve_replay_stream_with_config(
        scraper_agregator,
        credentials_store,
        normalized_video_id,
        stream_kind,
        ScraperHttpConfig::default(),
        endpoints,
    )
    .await
}

async fn resolve_replay_stream_with_config(
    scraper_agregator: &ScraperAgregator,
    credentials_store: &dyn CredentialsStore,
    normalized_video_id: &str,
    stream_kind: Option<String>,
    http_config: ScraperHttpConfig,
    endpoints: &PlayerResolverEndpoints,
) -> Result<ResolvedPlayerStream> {
    let http_client = scraper_agregator.create_http_client(http_config);
    let session = get_or_login_session(&http_client, credentials_store).await?;
    let media_info = fetch_media_info(&http_client, &session, normalized_video_id, false).await?;

    build_resolved_player_stream(
        &http_client,
        normalized_video_id,
        &media_info,
        stream_kind,
        endpoints,
    )
    .await
}

async fn resolve_live_stream(
    scraper_agregator: &ScraperAgregator,
    credentials_store: &dyn CredentialsStore,
    channel_id: &str,
    stream_kind: Option<String>,
    endpoints: &PlayerResolverEndpoints,
) -> Result<ResolvedPlayerStream> {
    let normalized_channel_id = channel_id.trim();
    if normalized_channel_id.is_empty() {
        bail!("Missing TF1 live channel identifier.");
    }

    for attempt in 1..=3 {
        match resolve_live_stream_with_config(
            scraper_agregator,
            credentials_store,
            normalized_channel_id,
            stream_kind.clone(),
            tf1_http_config(),
            endpoints,
        )
        .await
        {
            Ok(stream) => return Ok(stream),
            Err(error) if is_proxy_error(&error) => {
                tracing::warn!(
                    attempt,
                    error = %error,
                    "TF1 FR proxy failed while resolving live stream"
                );
                if attempt < 3 {
                    tokio::time::sleep(Duration::from_millis(250 * attempt)).await;
                    continue;
                }
            }
            Err(error) => return Err(error),
        }
    }

    tracing::warn!(
        "TF1 FR proxy retries exhausted while resolving live stream; retrying without FR proxy"
    );
    resolve_live_stream_with_config(
        scraper_agregator,
        credentials_store,
        normalized_channel_id,
        stream_kind,
        ScraperHttpConfig::default(),
        endpoints,
    )
    .await
}

async fn resolve_live_stream_with_config(
    scraper_agregator: &ScraperAgregator,
    credentials_store: &dyn CredentialsStore,
    normalized_channel_id: &str,
    stream_kind: Option<String>,
    http_config: ScraperHttpConfig,
    endpoints: &PlayerResolverEndpoints,
) -> Result<ResolvedPlayerStream> {
    let http_client = scraper_agregator.create_http_client(http_config);
    let session = get_or_login_session(&http_client, credentials_store).await?;
    let live_video_id = format!("L_{}", normalized_channel_id.to_uppercase());
    let media_info = fetch_media_info(&http_client, &session, &live_video_id, true).await?;

    build_resolved_player_stream(
        &http_client,
        &live_video_id,
        &media_info,
        stream_kind,
        endpoints,
    )
    .await
}

fn tf1_http_config() -> ScraperHttpConfig {
    ScraperHttpConfig::default().proxy_country(TF1_PROXY_COUNTRY)
}

fn is_proxy_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<ArachneaHttpError>(),
            Some(ArachneaHttpError::Proxy(_))
        )
    })
}

async fn build_resolved_player_stream(
    http_client: &HttpClient,
    video_id: &str,
    media_info: &Value,
    stream_kind: Option<String>,
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
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(String::from)
            .unwrap_or(format!("TF1 rejected playback for this video (http error {}).", delivery_code));
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
    let proxy_url =
        save_tf1_license_proxy_url(endpoints, &license_url, &license_headers, &stream_kind);

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
        .map(|url| proxied_tf1_storyboard_vtt_url(endpoints, url));

    let chapters = extract_chapters(media_info);

    Ok(ResolvedPlayerStream {
        stream_url: vec![proxied_url(
            &manifest_url,
            endpoints.http_proxy_public_path.as_deref(),
            None,
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

fn proxied_tf1_storyboard_vtt_url(endpoints: &PlayerResolverEndpoints, vtt_url: &str) -> String {
    let actions = [ReplaceAll::new(
        r"(?m)^\s*(/[^#\r\n]+)(#xywh=\d+,\d+,\d+,\d+)",
        "{base_url}$1$2",
        None,
    )];

    proxied_url(
        vtt_url,
        endpoints.http_proxy_public_path.as_deref(),
        None,
        &actions,
        &[],
    )
}

async fn get_or_login_session(
    http_client: &HttpClient,
    credentials_store: &dyn CredentialsStore,
) -> Result<Tf1Session> {
    let (login, password) = load_credentials(credentials_store)?;

    if let Some(session) = load_cached_session(&login) {
        return Ok(session);
    }

    let _login_guard = TF1_LOGIN_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    if let Some(session) = load_cached_session(&login) {
        return Ok(session);
    }

    bootstrap_gigya_session(http_client).await?;

    let login_body = serde_urlencoded::to_string([
        ("loginID", login.as_str()),
        ("password", password.as_str()),
        ("sessionExpiration", "31536000"),
        ("targetEnv", "jssdk"),
        ("include", "identities-all,data,profile,preferences,"),
        ("includeUserInfo", "true"),
        ("loginMode", "standard"),
        ("lang", "fr"),
        ("APIKey", TF1_GIGYA_API_KEY),
        ("sdk", "js_latest"),
        ("authMode", "cookie"),
        ("pageURL", TF1_BASE_URL),
        ("sdkBuild", "13987"),
        ("format", "json"),
    ])
    .context("Failed to serialize the TF1 login payload.")?;

    let login_headers = HashMap::from([
        (
            "content-type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        ),
        ("referer".to_string(), TF1_BASE_URL.to_string()),
        (
            "user-agent".to_string(),
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:148.0) Gecko/20100101 Firefox/148.0"
                .to_string(),
        ),
    ]);

    let login_payload = http_client
        .get_json_for_request(
            http::Method::POST,
            TF1_LOGIN_URL,
            &login_headers,
            Some(&login_body),
        )
        .await
        .context("Failed to submit TF1 credentials to Gigya.")?;
    bail_on_gigya_error(&login_payload, "TF1 rejected the configured credentials.")?;

    let uid = read_json_string(
        &login_payload,
        &["userInfo", "UID"],
        "Missing TF1 UID in Gigya login response.",
    )?;
    let signature = read_json_string(
        &login_payload,
        &["userInfo", "UIDSignature"],
        "Missing TF1 UID signature in Gigya login response.",
    )?;
    let timestamp = read_json_string(
        &login_payload,
        &["userInfo", "signatureTimestamp"],
        "Missing TF1 signature timestamp in Gigya login response.",
    )?;

    let token_body = json!({
        "uid": uid,
        "signature": signature,
        "timestamp": timestamp.parse::<u64>().unwrap_or_default(),
        "consent_ids": [
            "1", "2", "3", "4", "10001", "10003", "10005", "10007", "10013",
            "10015", "10017", "10019", "10009", "10011", "13002", "13001",
            "10004", "10014", "10016", "10018", "10020", "10010", "10012",
            "10006", "10008"
        ]
    })
    .to_string();

    let token_headers = HashMap::from([
        ("content-type".to_string(), "application/json".to_string()),
        (
            "user-agent".to_string(),
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:148.0) Gecko/20100101 Firefox/148.0"
                .to_string(),
        ),
    ]);

    let token_payload = http_client
        .get_json_for_request(
            http::Method::POST,
            TF1_TOKEN_URL,
            &token_headers,
            Some(&token_body),
        )
        .await
        .context("Failed to exchange TF1 Gigya identifiers for a TF1 web token.")?;
    let token = read_json_string(
        &token_payload,
        &["token"],
        "Missing TF1 bearer token in TF1 token response.",
    )?;

    let session = Tf1Session { token };
    save_cached_session(&login, &session);
    Ok(session)
}

async fn bootstrap_gigya_session(http_client: &HttpClient) -> Result<()> {
    let query = serde_urlencoded::to_string([
        ("apiKey", TF1_GIGYA_API_KEY),
        ("pageURL", "https://www.tf1.fr/"),
        ("sd", "js_latest"),
        ("sdkBuild", "13987"),
        ("format", "json"),
    ])
    .context("Failed to serialize the TF1 Gigya bootstrap query string.")?;
    let bootstrap_url = format!("{}?{}", TF1_BOOTSTRAP_URL, query);

    let bootstrap_headers = HashMap::from([
        ("referer".to_string(), TF1_BASE_URL.to_string()),
        (
            "user-agent".to_string(),
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:148.0) Gecko/20100101 Firefox/148.0"
                .to_string(),
        ),
    ]);

    http_client
        .query_http_for_request(http::Method::GET, &bootstrap_url, &bootstrap_headers, None)
        .await
        .context("Failed to bootstrap the TF1 Gigya web session.")?;

    Ok(())
}

async fn fetch_media_info(
    http_client: &HttpClient,
    session: &Tf1Session,
    video_id: &str,
    is_live: bool,
) -> Result<Value> {
    let video_url = TF1_MEDIA_INFO_URL_TEMPLATE.replace("{}", video_id.trim());
    let query = if is_live {
        serde_urlencoded::to_string([
            ("context", "MYTF1"),
            ("pver", "5029000"),
            ("platform", "web"),
            ("device", "desktop"),
            ("os", "windows"),
            ("osVersion", "10.0"),
            ("topDomain", "unknown"),
            ("playerVersion", "5.29.0"),
            ("productName", "mytf1"),
            ("productVersion", "3.37.0"),
        ])
    } else {
        serde_urlencoded::to_string([
            ("context", "MYTF1"),
            ("pver", "5010000"),
            ("platform", "web"),
            ("device", "desktop"),
            ("os", "linux"),
            ("osVersion", "unknown"),
            ("topDomain", TF1_BASE_URL),
            ("playerVersion", "5.19.0"),
            ("productName", "mytf1"),
            ("productVersion", "3.22.0"),
        ])
    }
    .context("Failed to serialize the TF1 mediainfo query string.")?;
    let url = format!("{}?{}", video_url, query);

    let headers = HashMap::from([
        (
            "accept".to_string(),
            "application/json, text/plain, */*".to_string(),
        ),
        (
            "authorization".to_string(),
            format!("Bearer {}", session.token),
        ),
        ("origin".to_string(), TF1_BASE_URL.to_string()),
        ("referer".to_string(), format!("{}/", TF1_BASE_URL)),
        (
            "user-agent".to_string(),
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:148.0) Gecko/20100101 Firefox/148.0"
                .to_string(),
        ),
    ]);

    let response = http_client
        .send_for_request(http::Method::GET, &url, &headers, None)
        .await
        .with_context(|| format!("Failed to fetch TF1 mediainfo for `{}`.", video_id))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .with_context(|| format!("Failed to read TF1 mediainfo response for `{}`.", video_id))?;

    if !status.is_success() {
        bail!(
            "TF1 mediainfo request for `{}` failed with HTTP {}: {}",
            video_id,
            status,
            preview_error_body(&body)
        );
    }

    serde_json::from_str(&body).with_context(|| {
        format!(
            "Invalid TF1 mediainfo JSON for `{}` (HTTP {}): {}",
            video_id,
            status,
            preview_error_body(&body)
        )
    })
}

fn bail_on_gigya_error(payload: &Value, default_message: &str) -> Result<()> {
    let error_code = payload
        .get("errorCode")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if error_code == 0 {
        return Ok(());
    }

    let message = payload
        .get("errorMessage")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            payload
                .get("errorDetails")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or(default_message);

    bail!(message.to_string())
}

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

fn manifest_type_from_url(url: &str) -> String {
    if url.trim().to_ascii_lowercase().contains(".m3u8") {
        return "m3u8".to_string();
    }

    "mpd".to_string()
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

fn preview_error_body(body: &str) -> String {
    let value = body.trim();
    if value.is_empty() {
        return "<empty body>".to_string();
    }

    let mut preview: String = value.chars().take(300).collect();
    if value.chars().count() > 300 {
        preview.push_str("...");
    }
    preview
}

fn load_credentials(credentials_store: &dyn CredentialsStore) -> Result<(String, String)> {
    let credentials = credentials_store
        .get_credentials(TF1_SERVICE_ID)
        .with_context(|| {
            format!(
                "Failed to load credentials for service `{}` from the configured store.",
                TF1_SERVICE_ID
            )
        })?
        .with_context(|| {
            format!(
                "Missing credentials for service `{}` in the configured store.",
                TF1_SERVICE_ID
            )
        })?;

    Ok((credentials.login, credentials.password))
}

fn load_cached_session(login: &str) -> Option<Tf1Session> {
    let cache = TF1_SESSION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache_guard) = cache.lock() else {
        return None;
    };

    let now = Instant::now();
    cache_guard.retain(|_, entry| entry.expires_at > now);
    cache_guard.get(login).map(|entry| entry.session.clone())
}

fn save_cached_session(login: &str, session: &Tf1Session) {
    let cache = TF1_SESSION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache_guard) = cache.lock() else {
        return;
    };

    cache_guard.insert(
        login.to_string(),
        CachedTf1Session {
            session: session.clone(),
            expires_at: Instant::now() + TF1_SESSION_TTL,
        },
    );
}

fn save_tf1_license_proxy_url(
    endpoints: &PlayerResolverEndpoints,
    license_url: &str,
    headers: &HashMap<String, String>,
    stream_kind: &str,
) -> String {
    let token = save_cached_license(license_url, headers, stream_kind);
    endpoints.drm_license_url(TF1_SERVICE_ID, &token)
}

fn save_cached_license(
    license_url: &str,
    headers: &HashMap<String, String>,
    stream_kind: &str,
) -> String {
    let cache = TF1_LICENSE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
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
        CachedTf1License {
            license_url: license_url.to_string(),
            headers: headers.clone(),
            stream_kind: stream_kind.to_string(),
            expires_at: now + TF1_LICENSE_TTL,
        },
    );

    token
}

fn load_cached_license(token: &str) -> Option<CachedTf1License> {
    let cache = TF1_LICENSE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache_guard) = cache.lock() else {
        return None;
    };

    let now = Instant::now();
    cache_guard.retain(|_, entry| entry.expires_at > now);
    cache_guard.get(token).cloned()
}

async fn proxy_tf1_license_request(
    scraper_agregator: &ScraperAgregator,
    token: &str,
    challenge_body: &[u8],
) -> Result<ProxiedStreamResponse> {
    for attempt in 1..=3 {
        match proxy_tf1_license_request_with_config(
            scraper_agregator,
            token,
            challenge_body,
            tf1_http_config(),
        )
        .await
        {
            Ok(response) => return Ok(response),
            Err(error) if is_proxy_error(&error) => {
                tracing::warn!(
                    attempt,
                    error = %error,
                    "TF1 FR proxy failed while proxying license request"
                );
                if attempt < 3 {
                    tokio::time::sleep(Duration::from_millis(250 * attempt)).await;
                    continue;
                }
            }
            Err(error) => return Err(error),
        }
    }

    tracing::warn!(
        "TF1 FR proxy retries exhausted while proxying license request; retrying without FR proxy"
    );
    proxy_tf1_license_request_with_config(
        scraper_agregator,
        token,
        challenge_body,
        ScraperHttpConfig::default(),
    )
    .await
}

async fn proxy_tf1_license_request_with_config(
    scraper_agregator: &ScraperAgregator,
    token: &str,
    challenge_body: &[u8],
    http_config: ScraperHttpConfig,
) -> Result<ProxiedStreamResponse> {
    let normalized_token = token.trim();
    if normalized_token.is_empty() {
        bail!("Missing TF1 stream token identifier.");
    }

    let cached =
        load_cached_license(normalized_token).context("Expired or missing TF1 stream token.")?;
    if cached.stream_kind != TF1_PROXY_STREAM_KIND {
        bail!("Unsupported TF1 stream kind `{}`.", cached.stream_kind);
    }

    let mut request_headers = HashMap::from([(
        "content-type".to_string(),
        "application/octet-stream".to_string(),
    )]);

    for (name, value) in &cached.headers {
        let trimmed_name = name.trim();
        let trimmed_value = value.trim();
        if trimmed_name.is_empty() || trimmed_value.is_empty() {
            continue;
        }

        request_headers.insert(trimmed_name.to_string(), trimmed_value.to_string());
    }

    let http_client = scraper_agregator.create_http_client(http_config);
    let response = http_client
        .send_bytes_for_request(
            http::Method::POST,
            cached.license_url.trim(),
            &request_headers,
            Some(challenge_body.to_vec()),
        )
        .await
        .context("Failed to call the TF1 Widevine license server.")?;

    let status = response.status();
    let content_type = response
        .headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string())
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let body = response
        .bytes()
        .await
        .context("Failed to read the TF1 license response.")?
        .to_vec();

    if !status.is_success() {
        let details = String::from_utf8(body)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("HTTP {}", status));
        bail!("TF1 license request failed: {}", details);
    }

    Ok(ProxiedStreamResponse {
        body,
        content_type,
        headers: HashMap::from([("cache-control".to_string(), "no-store".to_string())]),
    })
}
