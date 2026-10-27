//! TF1 license token caching and proxy requests.

use super::*;

pub(super) fn save_tf1_license_proxy_url(
    endpoints: &PlayerResolverEndpoints,
    license_url: &str,
    headers: &HashMap<String, String>,
    stream_kind: &str,
    proxy_countries: &[String],
) -> String {
    let token = save_cached_license(license_url, headers, stream_kind, proxy_countries);
    endpoints.drm_license_url(TF1_SERVICE_ID, &token)
}

pub(super) fn save_cached_license(
    license_url: &str,
    headers: &HashMap<String, String>,
    stream_kind: &str,
    proxy_countries: &[String],
) -> String {
    let cache = TF1_LICENSE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let token: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(crate::PLAYER_PROXY_TOKEN_LENGTH)
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
            proxy_countries: proxy_countries.to_vec(),
            expires_at: now + TF1_LICENSE_TTL,
        },
    );

    token
}

pub(super) fn load_cached_license(token: &str) -> Option<CachedTf1License> {
    let cache = TF1_LICENSE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache_guard) = cache.lock() else {
        return None;
    };

    let now = Instant::now();
    cache_guard.retain(|_, entry| entry.expires_at > now);
    cache_guard.get(token).cloned()
}

pub(super) async fn proxy_tf1_license_request(
    scraper_agregator: &ScraperAgregator,
    token: &str,
    challenge_body: &[u8],
) -> Result<ProxiedStreamResponse> {
    let proxy_countries = load_cached_license(token)
        .map(|cached| cached.proxy_countries)
        .context("Expired or missing TF1 stream token.")?;
    proxy_tf1_license_request_with_config(
        scraper_agregator,
        token,
        challenge_body,
        tf1_http_config(&proxy_countries),
    )
    .await
}

pub(super) async fn proxy_tf1_license_request_with_config(
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
