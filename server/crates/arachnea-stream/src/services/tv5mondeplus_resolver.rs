use anyhow::{bail, Context, Result};
use arachnea_proxy::http::proxy_service::proxied_url;
use async_trait::async_trait;
use rand::{distr::Alphanumeric, Rng};
use rquest::header::{HeaderValue, CONTENT_TYPE};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use url::Url;

use arachnea_core::persistence::CredentialsStore;
use arachnea_scrapyfy::{HttpClient, ScraperAgregator, ScraperQueryCollectionParameter};

use crate::services::player_resolver::{
    PlayerResolverEndpoints, PlayerStreamResolver, ProxiedStreamResponse, ResolvedPlayerStream,
};

const TV5MONDEPLUS_SERVICE_ID: &str = "tv5mondeplus-fr";
const TV5MONDEPLUS_AUTH_URL: &str =
    "https://api.tv5mondeplus.com/v1/customer/TV5MONDE/businessunit/TV5MONDEplus/auth/anonymous";
const TV5MONDEPLUS_ENTITLEMENT_URL_TEMPLATE: &str = "https://api.tv5mondeplus.com/v2/customer/TV5MONDE/businessunit/TV5MONDEplus/entitlement/{}/play";
const TV5MONDEPLUS_LICENSE_TTL: Duration = Duration::from_secs(15 * 60);
const TV5MONDEPLUS_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:148.0) Gecko/20100101 Firefox/148.0";

static TV5MONDEPLUS_LICENSE_CACHE: OnceLock<Mutex<HashMap<String, CachedTv5mondeplusLicense>>> =
    OnceLock::new();

#[derive(Clone)]
struct CachedTv5mondeplusLicense {
    license_url: String,
    expires_at: Instant,
}

/// TV5MONDE+ implementation of the protected RedBee playback resolver contract.
pub(crate) struct Tv5mondeplusResolver;

#[async_trait]
impl PlayerStreamResolver for Tv5mondeplusResolver {
    fn source_id(&self) -> &'static str {
        TV5MONDEPLUS_SERVICE_ID
    }

    fn resolver_ids(&self) -> &'static [&'static str] {
        &["tv5mondeplus-video"]
    }

    async fn get_stream(
        &self,
        scraper_agregator: &ScraperAgregator,
        _credentials_store: &dyn CredentialsStore,
        resolver: &str,
        target: &str,
        _service_parameters: &[ScraperQueryCollectionParameter],
        endpoints: &PlayerResolverEndpoints,
    ) -> Result<ResolvedPlayerStream> {
        if resolver.trim() != "tv5mondeplus-video" {
            bail!(
                "Unsupported player resolver `{}` for source `{}`.",
                resolver,
                TV5MONDEPLUS_SERVICE_ID
            );
        }

        resolve_tv5mondeplus_stream(scraper_agregator, target, endpoints).await
    }

    async fn get_drm_license(
        &self,
        _scraper_agregator: &ScraperAgregator,
        stream_token: &str,
        body: &[u8],
    ) -> Result<ProxiedStreamResponse> {
        proxy_tv5mondeplus_license_request(stream_token, body).await
    }
}

async fn resolve_tv5mondeplus_stream(
    scraper_agregator: &ScraperAgregator,
    asset_id: &str,
    endpoints: &PlayerResolverEndpoints,
) -> Result<ResolvedPlayerStream> {
    let asset_id = asset_id.trim();
    if asset_id.is_empty() {
        bail!("Missing TV5MONDE+ asset identifier.");
    }

    let http_client = scraper_agregator.create_http_client(Default::default());
    let device_id = random_device_id();
    let session_token = authenticate_anonymous(&http_client, &device_id).await?;
    let entitlement = fetch_entitlement(&http_client, asset_id, &device_id, &session_token).await?;
    let selected_format = select_best_format(&entitlement)
        .with_context(|| entitlement_error_message(&entitlement))?;

    let license_url = selected_format
        .license_url
        .as_deref()
        .map(|url| save_tv5mondeplus_license_proxy_url(endpoints, url));
    let storyboard_vtt_url = entitlement
        .pointer("/sprites/0/vtt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(|url| proxied_media_url(url, endpoints.http_proxy_public_path.as_deref()));

    Ok(ResolvedPlayerStream {
        stream_url: vec![proxied_media_url(
            &selected_format.media_locator,
            endpoints.http_proxy_public_path.as_deref(),
        )],
        manifest_type: Some(selected_format.manifest_type),
        license_url,
        license_headers: HashMap::new(),
        storyboard_vtt_url,
        ..Default::default()
    })
}

async fn authenticate_anonymous(http_client: &HttpClient, device_id: &str) -> Result<String> {
    let body = json!({
        "device": {
            "deviceId": device_id,
            "width": 1920,
            "height": 1080,
            "type": "WEB",
            "name": "Arachnea TV5MONDE+ Resolver"
        },
        "deviceId": device_id
    })
    .to_string();
    let response = http_client
        .get_json_for_request(
            http::Method::POST,
            TV5MONDEPLUS_AUTH_URL,
            &json_headers(),
            Some(&body),
        )
        .await
        .context("Failed to create an anonymous TV5MONDE+ playback session.")?;

    read_json_string(
        &response,
        &["sessionToken"],
        "Missing TV5MONDE+ anonymous session token.",
    )
}

async fn fetch_entitlement(
    http_client: &HttpClient,
    asset_id: &str,
    device_id: &str,
    session_token: &str,
) -> Result<Value> {
    let mut url = Url::parse(&TV5MONDEPLUS_ENTITLEMENT_URL_TEMPLATE.replace("{}", asset_id))
        .context("Invalid TV5MONDE+ entitlement URL.")?;
    url.query_pairs_mut()
        .append_pair("ifa", device_id)
        .append_pair("deviceType", "Desktop")
        .append_pair("width", "1920")
        .append_pair("height", "1080")
        .append_pair(
            "pageUrl",
            &format!("https://www.tv5mondeplus.com/player/{asset_id}"),
        )
        .append_pair("domain", "www.tv5mondeplus.com")
        .append_pair("mute", "false")
        .append_pair("autoplay", "true");

    let mut headers = json_headers();
    headers.insert(
        "authorization".to_string(),
        format!("Bearer {session_token}"),
    );
    headers.insert(
        "referer".to_string(),
        "https://www.tv5mondeplus.com/".to_string(),
    );
    headers.insert(
        "user-agent".to_string(),
        TV5MONDEPLUS_USER_AGENT.to_string(),
    );

    let response = http_client
        .get_json_for_request(http::Method::GET, url.as_str(), &headers, None)
        .await
        .context("Failed to fetch TV5MONDE+ playback entitlement.")?;

    if response.get("httpCode").and_then(Value::as_i64) == Some(403) {
        bail!("{}", entitlement_error_message(&response));
    }

    Ok(response)
}

struct SelectedTv5mondeplusFormat {
    media_locator: String,
    manifest_type: String,
    license_url: Option<String>,
}

fn select_best_format(entitlement: &Value) -> Option<SelectedTv5mondeplusFormat> {
    let formats = entitlement.get("formats")?.as_array()?;

    formats
        .iter()
        .find_map(|format| selected_format(format, "DASH", "mpd"))
        .or_else(|| {
            formats
                .iter()
                .find_map(|format| selected_format(format, "HLS", "m3u8"))
        })
}

fn selected_format(
    format: &Value,
    expected_format: &str,
    manifest_type: &str,
) -> Option<SelectedTv5mondeplusFormat> {
    if format.get("format")?.as_str()? != expected_format {
        return None;
    }

    let media_locator = format.get("mediaLocator")?.as_str()?.trim().to_string();
    if media_locator.is_empty() {
        return None;
    }

    Some(SelectedTv5mondeplusFormat {
        media_locator,
        manifest_type: manifest_type.to_string(),
        license_url: format
            .pointer("/drm/com.widevine.alpha/licenseServerUrl")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .map(str::to_string),
    })
}

fn entitlement_error_message(entitlement: &Value) -> String {
    entitlement
        .get("message")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .unwrap_or("Missing playable TV5MONDE+ DASH or HLS format.")
        .to_string()
}

fn proxied_media_url(media_locator: &str, proxy_path: Option<&str>) -> String {
    proxied_url(media_locator, proxy_path, None, &[], &[])
}

fn save_tv5mondeplus_license_proxy_url(
    endpoints: &PlayerResolverEndpoints,
    license_url: &str,
) -> String {
    let token_id = random_device_id();
    let cache = TV5MONDEPLUS_LICENSE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut cache) = cache.lock() {
        let now = Instant::now();
        cache.retain(|_, entry| entry.expires_at > now);
        cache.insert(
            token_id.clone(),
            CachedTv5mondeplusLicense {
                license_url: license_url.to_string(),
                expires_at: now + TV5MONDEPLUS_LICENSE_TTL,
            },
        );
    }
    endpoints.drm_license_url(TV5MONDEPLUS_SERVICE_ID, &token_id)
}

async fn proxy_tv5mondeplus_license_request(
    token_id: &str,
    challenge_body: &[u8],
) -> Result<ProxiedStreamResponse> {
    let cache = TV5MONDEPLUS_LICENSE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let license_url = {
        let mut cache = cache
            .lock()
            .map_err(|error| anyhow::anyhow!("TV5MONDE+ license cache lock failed: {error}"))?;
        let now = Instant::now();
        cache.retain(|_, entry| entry.expires_at > now);
        cache
            .get(token_id.trim())
            .map(|entry| entry.license_url.clone())
            .context("Expired or missing TV5MONDE+ license token.")?
    };

    let response = rquest::Client::builder()
        .user_agent(TV5MONDEPLUS_USER_AGENT)
        .timeout(Duration::from_secs(25))
        .build()
        .context("Failed to build the TV5MONDE+ license proxy client.")?
        .post(license_url)
        .header(
            CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        )
        .body(challenge_body.to_vec())
        .send()
        .await
        .context("Failed to call the TV5MONDE+ Widevine license server.")?;

    let status = response.status();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let body = response
        .bytes()
        .await
        .context("Failed to read the TV5MONDE+ license response.")?
        .to_vec();

    if !status.is_success() {
        let details = String::from_utf8(body)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("HTTP {status}"));
        bail!("TV5MONDE+ license request failed: {details}");
    }

    Ok(ProxiedStreamResponse {
        body,
        content_type,
        headers: HashMap::from([("cache-control".to_string(), "no-store".to_string())]),
    })
}

fn json_headers() -> HashMap<String, String> {
    HashMap::from([
        ("accept".to_string(), "application/json".to_string()),
        (
            "content-type".to_string(),
            "application/json;charset=utf-8".to_string(),
        ),
        (
            "user-agent".to_string(),
            TV5MONDEPLUS_USER_AGENT.to_string(),
        ),
    ])
}

fn random_device_id() -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

fn read_json_string(payload: &Value, path: &[&str], error_message: &str) -> Result<String> {
    let mut current = payload;
    for segment in path {
        current = current
            .get(*segment)
            .with_context(|| error_message.to_string())?;
    }
    current
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .with_context(|| error_message.to_string())
}
