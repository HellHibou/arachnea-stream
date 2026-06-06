use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};
use regex::Regex;
use rquest::{
    cookie::Jar,
    header::{HeaderMap, HeaderName, HeaderValue},
    redirect::Policy,
    Client, Response, StatusCode, Url,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use arachnea_core::persistence::CredentialsStore;
use arachnea_scrapyfy::{
    HttpClient, ScraperHttpConfig, ScraperHttpMode, ScraperQueryCollectionParameter,
};

use crate::services::player_resolver::{
    normalize_stream_kind, proxy_drm_today_license_request, save_drm_today_license_proxy_url,
    PlayerStreamResolver, ProxiedStreamResponse, ResolvedPlayerStream,
};

const RTLPLAY_SERVICE_ID: &str = "rtlplay-be";
const BASE_URL: &str = "https://www.rtlplay.be/rtlplay";
const URL_CONFIG_TEMPLATE: &str = "https://videoplayer-service.dpgmedia.net/play-config/{}";
const URL_SSO_LOGIN: &str = "https://sso.rtl.be/api/account/login";
const URL_SSO_AUTH: &str = "https://sso.rtl.be/oidc/account/authenticate";
const DEFAULT_LICENSE_URL: &str = "https://lic.drmtoday.com/license-proxy-widevine/cenc/";
const API_KEY: &str = "2W7kCXUTyUgKf7HKlK9qcYJvFmiPFaBEFT90eC2b";
const GIGYA_COOKIE_NAME: &str =
    "gig_bootstrap_3_LGnnaXIFQ_VRXofTaFTGnc6q7pM923yFB0AXSWdxADsUT0y2dVdDKmPRyQMj7LMc";
const GIGYA_COOKIE_VALUE: &str = "_gigya_ver4";
const POPCORN_SDK: &str = "8";
const RTLPLAY_CUSTOMER_NAME: &str = "rtlbe";
const RTLPLAY_SESSION_FALLBACK_TTL: Duration = Duration::from_secs(15 * 60);
const MAX_REDIRECTS: usize = 12;

static RTLPLAY_SESSION_CACHE: OnceLock<Mutex<HashMap<String, CachedRtlPlaySession>>> =
    OnceLock::new();
static NEXT_DATA_REGEX: OnceLock<Regex> = OnceLock::new();

#[derive(Clone)]
struct RtlPlaySession {
    client: Client,
    cookies: HashMap<String, String>,
    _cookie_jar: Arc<Jar>,
}

struct CachedRtlPlaySession {
    session: RtlPlaySession,
    expires_at: Instant,
}

struct ResolvedRtlPlayVideo {
    manifest_url: String,
    license_url: Option<String>,
    license_token: Option<String>,
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

    async fn resolve_player_stream(
        &self,
        credentials_store: &dyn CredentialsStore,
        resolver_kind: &str,
        resolver_target: &str,
        resolver_stream_kind: Option<String>,
        service_parameters: &[ScraperQueryCollectionParameter],
    ) -> Result<ResolvedPlayerStream> {
        match resolver_kind.trim() {
            "rtlplay-video" => {
                let api_version = RtlPlayApiVersion::from_parameters(service_parameters)?;
                resolve_replay_stream(
                    credentials_store,
                    resolver_target,
                    resolver_stream_kind,
                    &api_version,
                )
                .await
            }
            "rtlplay-live" => {
                let api_version = RtlPlayApiVersion::from_parameters(service_parameters)?;
                resolve_live_stream(
                    credentials_store,
                    resolver_target,
                    resolver_stream_kind,
                    &api_version,
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

    async fn get_stream(&self, stream_token: &str, body: &[u8]) -> Result<ProxiedStreamResponse> {
        proxy_drm_today_license_request(stream_token, body).await
    }
}

async fn resolve_replay_stream(
    credentials_store: &dyn CredentialsStore,
    video_id: &str,
    stream_kind: Option<String>,
    api_version: &RtlPlayApiVersion,
) -> Result<ResolvedPlayerStream> {
    let normalized_video_id = video_id.trim();
    if normalized_video_id.is_empty() {
        bail!("Missing RTL Play video identifier.");
    }

    let session = get_or_login_session(credentials_store).await?;
    let video_url = format!("{}/player/{}", BASE_URL, normalized_video_id);
    let resolved = resolve_final_video_url(&session, &video_url, false, api_version).await?;

    build_resolved_player_stream(resolved, stream_kind)
}

async fn resolve_live_stream(
    credentials_store: &dyn CredentialsStore,
    channel_id: &str,
    stream_kind: Option<String>,
    api_version: &RtlPlayApiVersion,
) -> Result<ResolvedPlayerStream> {
    let normalized_channel = normalize_live_channel(channel_id)?;
    let session = get_or_login_session(credentials_store).await?;
    let video_url = format!("{}/direct/{}", BASE_URL, normalized_channel.as_str());
    let resolved = resolve_final_video_url(&session, &video_url, true, api_version).await?;

    build_resolved_player_stream(resolved, stream_kind)
}

fn build_resolved_player_stream(
    resolved: ResolvedRtlPlayVideo,
    stream_kind: Option<String>,
) -> Result<ResolvedPlayerStream> {
    let license_url = resolved.license_token.as_deref().map(|license_token| {
        let stream_kind = normalize_stream_kind(stream_kind);
        save_drm_today_license_proxy_url(
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
        stream_url: resolved.manifest_url,
        manifest_type: "mpd".to_string(),
        license_url,
        license_headers: HashMap::new(),
    })
}

async fn get_or_login_session(credentials_store: &dyn CredentialsStore) -> Result<RtlPlaySession> {
    let (login, password) = load_credentials(credentials_store)?;

    if let Some(session) = load_cached_session(&login) {
        return Ok(session);
    }

    let (client, cookie_jar) = build_client()?;
    let base_url = Url::parse(BASE_URL).context("Invalid RTL Play base URL.")?;
    let sso_base_url = Url::parse("https://sso.rtl.be/").context("Invalid RTL Play SSO URL.")?;
    let mut seen_cookies = HashMap::new();

    let response = client
        .get(BASE_URL)
        .headers(generic_headers()?)
        .send()
        .await
        .context("Failed to fetch the RTL Play root page before login.")?;
    collect_response_cookies(&response, &mut seen_cookies);
    ensure_success(response, "RTL Play root page").await?;

    let lfvp_device_id = seen_cookies
        .get("lfvp_device_id")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .context("Missing RTL Play device cookie after root page warm-up.")?;
    cookie_jar.add_cookie_str(
        &format!("lfvp_device_id={}; Path=/", lfvp_device_id),
        &base_url,
    );
    cookie_jar.add_cookie_str(
        &format!("lfvp_device_id={}; Path=/", lfvp_device_id),
        &sso_base_url,
    );
    cookie_jar.add_cookie_str(
        &format!("lfvp_auth.redirect_uri={}; Path=/", BASE_URL),
        &base_url,
    );
    cookie_jar.add_cookie_str(
        &format!("{}={}; Path=/", GIGYA_COOKIE_NAME, GIGYA_COOKIE_VALUE),
        &sso_base_url,
    );

    let redirect_urls = get_following_redirects(
        &client,
        &format!("{}/connexion", BASE_URL),
        generic_headers()?,
        &mut seen_cookies,
    )
    .await
    .context("Failed to follow the RTL Play SSO redirect.")?;

    let sso_redirect_urls = sso_redirect_urls_from_chain(&redirect_urls);
    if sso_redirect_urls.is_empty() {
        bail!("Missing RTL Play SSO redirect URL.");
    }

    let response = client
        .post(URL_SSO_LOGIN)
        .headers(generic_headers()?)
        .json(&json!({
            "username": login,
            "password": password,
        }))
        .send()
        .await
        .context("Failed to submit RTL Play credentials.")?;
    collect_response_cookies(&response, &mut seen_cookies);
    let login_payload: Value = response
        .json()
        .await
        .context("Invalid RTL Play SSO login JSON response.")?;

    if login_payload.get("httpStatusCode").and_then(Value::as_i64) == Some(400) {
        bail!("RTL Play rejected the configured credentials.");
    }

    let sso_token = read_json_string(
        &login_payload,
        &["data", "userAccount", "session", "encryptedToken"][..],
        "Missing RTL Play SSO token in login response.",
    )?;

    for sso_redirect_url in &sso_redirect_urls {
        let auth_request = client
            .get(URL_SSO_AUTH)
            .headers(generic_headers()?)
            .query(&[
                ("redirectUrl", sso_redirect_url.as_str()),
                ("token", sso_token.as_str()),
            ]);

        let response = auth_request
            .send()
            .await
            .context("Failed to authenticate the RTL Play SSO session.")?;
        follow_response_redirects(&client, response, &mut seen_cookies)
            .await
            .context("Failed to complete the RTL Play SSO authentication redirects.")?;

        if seen_cookies
            .get("lfvp_rtlplay_auth")
            .map(String::as_str)
            .filter(|value| !value.trim().is_empty())
            .is_some()
        {
            break;
        }
    }

    let auth_cookie = seen_cookies
        .get("lfvp_rtlplay_auth")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .with_context(|| {
            format!(
                "RTL Play login completed without an lfvp_rtlplay_auth cookie after {} redirect candidate(s). Seen cookies: {}.",
                sso_redirect_urls.len(),
                cookie_name_summary(&seen_cookies),
            )
        })?;
    let expires_at = jwt_expires_at(auth_cookie)
        .or_else(|| {
            seen_cookies
                .get("lfvp_access_token")
                .and_then(|token| jwt_expires_at(token))
        })
        .unwrap_or_else(|| Instant::now() + RTLPLAY_SESSION_FALLBACK_TTL);

    let session = RtlPlaySession {
        client,
        cookies: seen_cookies,
        _cookie_jar: cookie_jar,
    };
    save_cached_session(&login, session.clone(), expires_at);

    Ok(session)
}

async fn resolve_final_video_url(
    session: &RtlPlaySession,
    video_url: &str,
    is_live: bool,
    api_version: &RtlPlayApiVersion,
) -> Result<ResolvedRtlPlayVideo> {
    HttpClient::store_cookies_for_url(BASE_URL, &rtlplay_player_cookies(&session.cookies)).await?;
    let http_client = HttpClient::with_http_config(ScraperHttpConfig {
        mode: Some(ScraperHttpMode::Direct),
        user_agent_profile: None,
        user_agent: Some(rtlplay_user_agent(api_version)),
        max_redirects: Some(MAX_REDIRECTS),
    });
    let html = http_client
        .query_http_for_request(
            http::Method::GET,
            video_url,
            &header_map_to_hash_map(&rtlplay_headers(api_version)?),
            None,
        )
        .await
        .context("Failed to fetch RTL Play player HTML.")?;

    let next_data =
        extract_next_data_json(&html).context("Missing RTL Play __NEXT_DATA__ payload.")?;
    let page_props = next_data
        .pointer("/props/pageProps")
        .context("Missing RTL Play page props in __NEXT_DATA__.")?;
    let bearer_token = read_json_string(
        page_props,
        &["authToken"][..],
        "Missing RTL Play player bearer token.",
    )?;
    let content_id = if is_live {
        extract_live_content_id(page_props).context("Missing RTL Play live content identifier.")?
    } else {
        read_json_string(
            page_props,
            &["id"][..],
            "Missing RTL Play replay content identifier.",
        )
        .or_else(|_| replay_content_id_from_url(video_url))
        .context("Missing RTL Play replay content identifier.")?
    };

    let config_url = format!(
        "{}?startPosition=0.0&autoPlay=true",
        URL_CONFIG_TEMPLATE.replace("{}", &content_id)
    );
    let mut headers = rtlplay_headers(api_version)?;
    headers.insert(
        HeaderName::from_static("x-api-key"),
        HeaderValue::from_static(API_KEY),
    );
    headers.insert(
        HeaderName::from_static("popcorn-sdk-version"),
        HeaderValue::from_static(POPCORN_SDK),
    );
    headers.insert(
        HeaderName::from_static("authorization"),
        HeaderValue::from_str(&format!("Bearer {}", bearer_token))
            .context("Invalid RTL Play bearer authorization header.")?,
    );

    let body = json!({
        "deviceType": "android-phone",
        "zone": "rtlplay",
    });

    let response = session
        .client
        .post(&config_url)
        .headers(headers)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("Failed to fetch RTL Play play-config `{}`.", config_url))?;

    if response.status() == StatusCode::FORBIDDEN {
        bail!("RTL Play play-config returned HTTP 403.");
    }

    let payload: Value = response
        .json()
        .await
        .context("Invalid RTL Play play-config JSON response.")?;

    if is_unavailable_payload(&payload) {
        bail!("RTL Play content is not currently available.");
    }

    select_dash_stream(&payload).context("Missing RTL Play DASH stream in play-config response.")
}

fn select_dash_stream(payload: &Value) -> Option<ResolvedRtlPlayVideo> {
    let streams = payload.get("video")?.get("streams")?.as_array()?;

    for stream in streams {
        let stream_type = stream.get("type").and_then(Value::as_str)?;
        let manifest_url = stream.get("url").and_then(Value::as_str)?;
        if stream_type != "dash" || !manifest_url.contains(".mpd") {
            continue;
        }

        let drm = stream.get("drm").and_then(Value::as_object);
        let widevine = drm.and_then(|drm| {
            drm.iter()
                .find(|(key, _)| key.to_ascii_lowercase().contains("widevine"))
                .map(|(_, value)| value)
        });
        let license_token = widevine
            .and_then(|value| value.pointer("/drmtoday/authToken"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let license_url = widevine
            .and_then(|value| value.get("licenseUrl"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        return Some(ResolvedRtlPlayVideo {
            manifest_url: manifest_url.to_string(),
            license_url,
            license_token,
        });
    }

    None
}

async fn get_following_redirects(
    client: &Client,
    url: &str,
    headers: HeaderMap,
    cookies: &mut HashMap<String, String>,
) -> Result<Vec<String>> {
    let response = client
        .get(url)
        .headers(headers)
        .send()
        .await
        .with_context(|| format!("Failed to fetch `{}`.", url))?;

    follow_response_redirects(client, response, cookies).await
}

async fn follow_response_redirects(
    client: &Client,
    mut response: Response,
    cookies: &mut HashMap<String, String>,
) -> Result<Vec<String>> {
    let mut urls = Vec::new();

    for _ in 0..12 {
        collect_response_cookies(&response, cookies);
        urls.push(response.url().to_string());

        if !response.status().is_redirection() {
            ensure_success(response, "RTL Play redirected request").await?;
            return Ok(urls);
        }

        let location = response
            .headers()
            .get(rquest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .context("RTL Play redirect response has no Location header.")?;
        let next_url = response
            .url()
            .join(location)
            .context("Invalid RTL Play redirect Location header.")?;

        response = client
            .get(next_url)
            .headers(generic_headers()?)
            .send()
            .await
            .context("Failed to follow RTL Play redirect.")?;
    }

    bail!("RTL Play redirect chain is too long.")
}

async fn ensure_success(response: Response, context: &str) -> Result<()> {
    if response.status().is_success() || response.status().is_redirection() {
        return Ok(());
    }

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let details = body.trim();
    if details.is_empty() {
        bail!("{} returned HTTP {}.", context, status);
    }

    bail!("{} returned HTTP {}: {}", context, status, details);
}

fn collect_response_cookies(response: &Response, cookies: &mut HashMap<String, String>) {
    for cookie in response.cookies() {
        cookies.insert(cookie.name().to_string(), cookie.value().to_string());
    }
}

fn rtlplay_player_cookies(cookies: &HashMap<String, String>) -> HashMap<String, String> {
    cookies
        .iter()
        .filter_map(|(name, value)| {
            matches!(
                name.as_str(),
                "lfvp_device_id"
                    | "lfvp_rtlplay_auth"
                    | "lfvp_access_token"
                    | "lfvp_auth.redirect_uri"
            )
            .then(|| (name.clone(), value.clone()))
        })
        .collect()
}

fn cookie_name_summary(cookies: &HashMap<String, String>) -> String {
    let mut names = cookies
        .keys()
        .map(String::as_str)
        .filter(|name| !name.trim().is_empty())
        .collect::<Vec<_>>();
    names.sort_unstable();
    if names.is_empty() {
        "none".to_string()
    } else {
        names.join(", ")
    }
}

fn header_map_to_hash_map(headers: &HeaderMap) -> HashMap<String, String> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect()
}

fn build_client() -> Result<(Client, Arc<Jar>)> {
    let cookie_jar = Arc::new(Jar::default());
    let client = Client::builder()
        .cookie_provider(Arc::clone(&cookie_jar))
        .redirect(Policy::none())
        .timeout(Duration::from_secs(25))
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:128.0) Gecko/20100101 Firefox/128.0",
        )
        .build()
        .context("Failed to build the RTL Play HTTP client.")?;

    Ok((client, cookie_jar))
}

fn generic_headers() -> Result<HeaderMap> {
    header_map(
        &[
            (
                "user-agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:128.0) Gecko/20100101 Firefox/128.0",
            ),
            ("accept", "*/*"),
            ("accept-language", "fr,fr-FR;q=0.8,en-US;q=0.5,en;q=0.3"),
            ("connection", "keep-alive"),
            ("upgrade-insecure-requests", "1"),
            ("sec-fetch-dest", "document"),
            ("sec-fetch-mode", "navigate"),
            ("sec-fetch-site", "none"),
            ("sec-fetch-user", "?1"),
            ("sec-gpc", "1"),
            ("priority", "u=0, i"),
        ][..],
    )
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

fn header_map(headers: &[(&str, &str)]) -> Result<HeaderMap> {
    let mut map = HeaderMap::new();
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .with_context(|| format!("Invalid RTL Play header name `{}`.", name))?;
        let value = HeaderValue::from_str(value)
            .with_context(|| format!("Invalid RTL Play header value for `{}`.", name))?;
        map.insert(name, value);
    }

    Ok(map)
}

fn sso_redirect_path(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    if parsed.host_str()? != "sso.rtl.be" {
        return None;
    }

    if let Some(callback) = parsed.fragment().and_then(sso_oidc_callback_from_fragment) {
        return Some(callback);
    }

    if parsed.path() == "/" && parsed.query().is_none() {
        return None;
    }

    Some(url_path_with_query(&parsed))
}

fn sso_redirect_urls_from_chain(urls: &[String]) -> Vec<String> {
    let mut candidates = Vec::new();

    if let Some(candidate) = urls.iter().rev().find_map(|url| {
        Url::parse(url)
            .ok()
            .and_then(|parsed| parsed.fragment().and_then(sso_oidc_callback_from_fragment))
    }) {
        push_unique(&mut candidates, candidate);
    }

    for candidate in urls.iter().filter_map(|url| sso_redirect_path(url)) {
        push_unique(&mut candidates, candidate);
    }

    candidates
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn sso_oidc_callback_from_fragment(fragment: &str) -> Option<String> {
    let query = fragment.split_once('?')?.1;
    let params = serde_urlencoded::from_str::<HashMap<String, String>>(query).ok()?;
    let callback = params.get("oidc_callback")?.trim();
    if callback.is_empty() {
        return None;
    }

    Some(normalize_sso_redirect_path(callback))
}

fn normalize_sso_redirect_path(value: &str) -> String {
    Url::parse(value)
        .ok()
        .filter(|url| url.host_str() == Some("sso.rtl.be"))
        .map(|url| url_path_with_query(&url))
        .unwrap_or_else(|| {
            if value.starts_with('/') {
                value.to_string()
            } else {
                format!("/{value}")
            }
        })
}

fn url_path_with_query(url: &Url) -> String {
    match url.query() {
        Some(query) if !query.is_empty() => format!("{}?{}", url.path(), query),
        _ => url.path().to_string(),
    }
}

fn extract_next_data_json(html: &str) -> Option<Value> {
    let json = next_data_regex().captures(html)?.get(1)?.as_str().trim();

    serde_json::from_str(json).ok()
}

fn next_data_regex() -> &'static Regex {
    NEXT_DATA_REGEX.get_or_init(|| {
        Regex::new(r#"(?s)<script[^>]*\bid=["']__NEXT_DATA__["'][^>]*>(.*?)</script>"#)
            .expect("valid NEXT_DATA regex")
    })
}

fn extract_live_content_id(page_props: &Value) -> Option<String> {
    if let (Some(current_path), Some(channels)) = (
        page_props
            .get("currentPath")
            .and_then(json_scalar_to_string),
        page_props.get("channels").and_then(Value::as_array),
    ) {
        for channel in channels {
            let channel_url = channel.get("url").and_then(json_scalar_to_string);
            if channel_url.as_deref() == Some(current_path.as_str()) {
                return channel.get("id").and_then(json_scalar_to_string);
            }
        }
    }

    page_props
        .pointer("/playerData/assetId")
        .and_then(json_scalar_to_string)
        .or_else(|| {
            page_props
                .pointer("/channel/id")
                .and_then(json_scalar_to_string)
        })
}

fn replay_content_id_from_url(video_url: &str) -> Result<String> {
    video_url
        .split("/player/")
        .nth(1)
        .and_then(|value| value.split(['?', '/']).next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .context("Missing RTL Play replay content identifier in player URL.")
}

fn normalize_live_channel(channel_id: &str) -> Result<String> {
    let trimmed_channel = channel_id.trim();
    let channel_slug =
        live_channel_slug_from_url(trimmed_channel).unwrap_or_else(|| trimmed_channel.to_string());
    let normalized_channel = match channel_slug.as_str() {
        "rtl_tvi" => "tvi",
        "club_rtl" => "club",
        "plug_rtl" => "plug",
        "rtl_info" => "rtl_info",
        "rtl_sport" => "rtl_sport",
        "bel_rtl" => "bel",
        "contact" => "contact",
        "rtl_play" => "rtlplay",
        "rtl_district" => "RTLdistrict",
        "tvi" | "club" | "plug" | "bel" | "rtlplay" | "RTLdistrict" => channel_slug.as_str(),
        _ => "",
    };

    if normalized_channel.is_empty() {
        bail!("Unsupported RTL Play live channel `{}`.", channel_id);
    }

    Ok(normalized_channel.to_string())
}

fn live_channel_slug_from_url(channel_id: &str) -> Option<String> {
    let url = Url::parse(channel_id).ok()?;
    if url.domain() != Some("www.rtlplay.be") {
        return None;
    }

    url.path_segments()?
        .filter(|segment| !segment.is_empty())
        .last()
        .map(str::to_string)
}

fn is_unavailable_payload(payload: &Value) -> bool {
    let code = payload.get("code").and_then(Value::as_i64);
    let payload_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();

    (code == Some(103) && payload_type.contains("available"))
        || (code == Some(104) && payload_type.contains("found"))
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

fn load_credentials(credentials_store: &dyn CredentialsStore) -> Result<(String, String)> {
    let credentials = credentials_store
        .get_credentials(RTLPLAY_SERVICE_ID)
        .with_context(|| {
            format!(
                "Failed to load credentials for service `{}` from the configured store.",
                RTLPLAY_SERVICE_ID
            )
        })?
        .with_context(|| {
            format!(
                "Missing credentials for service `{}` in the configured store.",
                RTLPLAY_SERVICE_ID
            )
        })?;

    Ok((credentials.login, credentials.password))
}

fn load_cached_session(login: &str) -> Option<RtlPlaySession> {
    let cache = RTLPLAY_SESSION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache_guard) = cache.lock() else {
        return None;
    };

    let now = Instant::now();
    cache_guard.retain(|_, entry| entry.expires_at > now);
    cache_guard.get(login).map(|entry| entry.session.clone())
}

fn save_cached_session(login: &str, session: RtlPlaySession, expires_at: Instant) {
    let cache = RTLPLAY_SESSION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache_guard) = cache.lock() else {
        return;
    };

    cache_guard.insert(
        login.to_string(),
        CachedRtlPlaySession {
            session,
            expires_at,
        },
    );
}

fn jwt_expires_at(token: &str) -> Option<Instant> {
    let payload = token.split('.').nth(1)?;
    let decoded = general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| general_purpose::URL_SAFE.decode(payload))
        .ok()?;
    let payload: Value = serde_json::from_slice(&decoded).ok()?;
    let exp = payload.get("exp")?.as_u64()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();

    if exp <= now + 60 {
        return None;
    }

    Some(Instant::now() + Duration::from_secs(exp - now - 60))
}
