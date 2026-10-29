//! RTLplay authentication redirects, cookies and session cache.

use super::*;

/// Returns a cached RTLplay session or performs the full SSO login flow.
///
/// Cached sessions are keyed by the configured login. A process-wide lock
/// serializes cache misses so concurrent calls do not start duplicate logins.
///
/// # Arguments
/// * `http_client` - HTTP client used for warm-up, redirect and login requests.
/// * `credentials_store` - Store providing the RTLplay login and password.
///
/// # Errors
/// Returns an error when credentials are missing, the warm-up or SSO flow
/// fails, the device cookie is absent, or the resulting session cannot be
/// cached.
///
/// # Returns
/// A session containing the cookies required by player and media requests.
pub(super) async fn get_or_login_session(
    http_client: &HttpClient,
    credentials_store: &dyn CredentialsStore,
) -> Result<RtlPlaySession> {
    let (login, password) = load_credentials(credentials_store)?;

    if let Some(session) = load_cached_session(&login) {
        return Ok(session);
    }

    let _login_guard = RTLPLAY_LOGIN_LOCK
        .get_or_init(|| AsyncMutex::new(()))
        .lock()
        .await;

    if let Some(session) = load_cached_session(&login) {
        return Ok(session);
    }

    let mut seen_cookies = HashMap::new();

    // Warm-up: fetch the RTL Play root page to obtain initial cookies
    http_client
        .send_for_request(http::Method::GET, BASE_URL, &generic_hash_headers(), None)
        .await
        .context("Failed to fetch the RTL Play root page before login.")?;
    merge_seen_cookies_for_url(http_client, BASE_URL, &mut seen_cookies).await?;

    let lfvp_device_id = seen_cookies
        .get("lfvp_device_id")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .context("Missing RTL Play device cookie after root page warm-up.")?;

    seen_cookies.insert("lfvp_auth.redirect_uri".to_string(), BASE_URL.to_string());
    http_client
        .store_cookies_for_url_in_client(BASE_URL, &seen_cookies)
        .await?;
    http_client
        .store_cookies_for_url_in_client(
            SSO_BASE_URL,
            &HashMap::from([
                ("lfvp_device_id".to_string(), lfvp_device_id),
                (
                    GIGYA_COOKIE_NAME.to_string(),
                    GIGYA_COOKIE_VALUE.to_string(),
                ),
            ]),
        )
        .await?;

    // Fetch the SSO redirect URLs
    let connexion_url = format!("{}/connexion", BASE_URL);
    let redirect_response = http_client
        .send_for_request(
            http::Method::GET,
            &connexion_url,
            &generic_hash_headers(),
            None,
        )
        .await
        .context("Failed to follow the RTL Play SSO redirect.")?;
    merge_seen_cookies_for_url(http_client, BASE_URL, &mut seen_cookies).await?;
    merge_seen_cookies_for_url(http_client, SSO_BASE_URL, &mut seen_cookies).await?;

    let redirect_response_url = redirect_response.url().to_string();
    let redirect_urls = redirect_response.redirect_urls().to_vec();
    let redirect_response_body = redirect_response
        .text()
        .await
        .context("Failed to decode the RTL Play connexion page.")?;
    let sso_redirect_urls = extract_sso_urls(
        &redirect_response_url,
        &redirect_urls,
        &redirect_response_body,
    );
    if sso_redirect_urls.is_empty() {
        bail!("Missing RTL Play SSO redirect URL.");
    }

    // Submit login credentials
    let login_body = json!({
        "username": login,
        "password": password,
    })
    .to_string();
    let mut login_headers = sso_hash_headers();
    login_headers.insert("content-type".to_string(), "application/json".to_string());
    login_headers.insert("accept".to_string(), "application/json".to_string());
    login_headers.insert(
        "origin".to_string(),
        SSO_BASE_URL.trim_end_matches('/').to_string(),
    );

    let login_response = http_client
        .get_json_for_request(
            http::Method::POST,
            URL_SSO_LOGIN,
            &login_headers,
            Some(&login_body),
        )
        .await
        .context("Failed to submit RTL Play credentials.")?;

    if login_response.get("httpStatusCode").and_then(Value::as_i64) == Some(400) {
        bail!("RTL Play rejected the configured credentials.");
    }

    let sso_token = read_json_string(
        &login_response,
        &["data", "userAccount", "session", "encryptedToken"][..],
        "Missing RTL Play SSO token in login response.",
    )?;

    // Authenticate via each redirect URL until we get the auth cookie
    for sso_redirect_url in &sso_redirect_urls {
        let auth_url = Url::parse_with_params(
            URL_SSO_AUTH,
            &[
                ("redirectUrl", sso_redirect_url.as_str()),
                ("token", sso_token.as_str()),
            ],
        )
        .context("Invalid RTL Play SSO auth URL.")?
        .to_string();

        let auth_response = http_client
            .send_for_request(http::Method::GET, &auth_url, &sso_hash_headers(), None)
            .await
            .context("Failed to authenticate the RTL Play SSO session.")?;
        merge_seen_cookies_for_url(http_client, BASE_URL, &mut seen_cookies).await?;
        merge_seen_cookies_for_url(http_client, SSO_BASE_URL, &mut seen_cookies).await?;
        for redirect_url in auth_response.redirect_urls() {
            merge_seen_cookies_for_url(http_client, redirect_url, &mut seen_cookies).await?;
        }
        merge_seen_cookies_for_url(http_client, auth_response.url(), &mut seen_cookies).await?;

        if seen_cookies
            .get("lfvp_rtlplay_auth")
            .map(String::as_str)
            .filter(|value| !value.trim().is_empty())
            .is_some()
        {
            break;
        }

        if seen_cookies
            .get("CRTL_SESSION")
            .map(String::as_str)
            .filter(|value| !value.trim().is_empty())
            .is_none()
        {
            continue;
        }

        let post_auth_redirects =
            extract_sso_urls_from_redirects(auth_response.url(), auth_response.redirect_urls());
        for authorize_candidate in post_auth_redirects
            .iter()
            .chain(std::iter::once(sso_redirect_url))
        {
            let Some(authorize_url) = sso_authorize_url_from_redirect(authorize_candidate) else {
                continue;
            };
            let authorize_response = http_client
                .send_for_request(http::Method::GET, &authorize_url, &sso_hash_headers(), None)
                .await
                .context("Failed to complete the RTL Play OIDC authorization redirect.")?;
            merge_seen_cookies_for_url(http_client, BASE_URL, &mut seen_cookies).await?;
            merge_seen_cookies_for_url(http_client, SSO_BASE_URL, &mut seen_cookies).await?;
            for redirect_url in authorize_response.redirect_urls() {
                merge_seen_cookies_for_url(http_client, redirect_url, &mut seen_cookies).await?;
            }
            merge_seen_cookies_for_url(http_client, authorize_response.url(), &mut seen_cookies)
                .await?;

            if seen_cookies
                .get("lfvp_rtlplay_auth")
                .map(String::as_str)
                .filter(|value| !value.trim().is_empty())
                .is_some()
            {
                break;
            }
        }

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
        cookies: seen_cookies,
    };
    save_cached_session(&login, session.clone(), expires_at);

    Ok(session)
}

/// Merges the HTTP client's cookies for `url` into `cookies`.
///
/// # Arguments
/// * `http_client` - Client owning the cookie jar to read.
/// * `url` - URL whose cookie scope should be queried.
/// * `cookies` - Accumulator updated with the scoped cookies.
///
/// # Errors
/// Returns an error when the client cannot read cookies for the URL.
pub(super) async fn merge_seen_cookies_for_url(
    http_client: &HttpClient,
    url: &str,
    cookies: &mut HashMap<String, String>,
) -> Result<()> {
    cookies.extend(http_client.cookies_for_url_in_client(url).await?);
    Ok(())
}

/// Extracts SSO redirect URLs from the connexion flow.
///
/// The `__NEXT_DATA__` payload is preferred. When it is unavailable or does
/// not contain `ssoUrls`, the function falls back to matching SSO URLs in the
/// response body and redirect history.
///
/// # Arguments
/// * `response_url` - Final URL of the connexion response.
/// * `redirect_urls` - URLs visited while following the connexion redirects.
/// * `response` - Decoded body of the final connexion response.
///
/// # Returns
/// Unique SSO URLs in discovery order. The result may be empty when neither
/// the payload nor the fallbacks expose an SSO redirect.
pub(super) fn extract_sso_urls(
    response_url: &str,
    redirect_urls: &[String],
    response: &str,
) -> Vec<String> {
    let re = NEXT_DATA_REGEX.get_or_init(|| {
        Regex::new(r#"(?s)<script[^>]*\bid=["']__NEXT_DATA__["'][^>]*>(.*?)</script>"#)
            .expect("valid NEXT_DATA regex")
    });

    let Some(captures) = re.captures(response) else {
        // Fallback: look for SSO redirect URLs in the page
        let mut urls = Vec::new();
        for line in response.lines() {
            if line.contains("sso.rtl.be") && line.contains("oidc_callback") {
                if let Some(url) = extract_sso_url_from_line(line) {
                    urls.push(url);
                }
            }
        }
        if !urls.is_empty() {
            return urls;
        }

        return extract_sso_urls_from_redirects(response_url, redirect_urls);
    };

    let json_str = captures.get(1).map(|m| m.as_str()).unwrap_or("");
    if let Ok(next_data) = serde_json::from_str::<Value>(json_str) {
        let props = next_data.get("props").and_then(|v| v.get("pageProps"));
        if let Some(urls) = props
            .and_then(|p| p.get("ssoUrls"))
            .and_then(Value::as_array)
        {
            return urls
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }
    }

    extract_sso_urls_from_redirects(response_url, redirect_urls)
}

pub(super) fn extract_sso_url_from_line(line: &str) -> Option<String> {
    let re = Regex::new(r#""(https?://[^"]*sso\.rtl\.be[^"]*)""#).ok()?;
    re.captures(line)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

pub(super) fn extract_sso_urls_from_redirects(
    response_url: &str,
    redirect_urls: &[String],
) -> Vec<String> {
    let mut urls = Vec::new();

    if let Some(candidate) = redirect_urls.iter().rev().find_map(|redirect_url| {
        Url::parse(redirect_url)
            .ok()
            .and_then(|url| url.fragment().and_then(sso_oidc_callback_from_fragment))
    }) {
        push_unique(&mut urls, candidate);
    }

    for redirect_url in redirect_urls {
        push_sso_redirect_urls(redirect_url, &mut urls);
    }
    push_sso_redirect_urls(response_url, &mut urls);

    urls
}

pub(super) fn push_sso_redirect_urls(source_url: &str, urls: &mut Vec<String>) {
    let Ok(url) = Url::parse(source_url) else {
        return;
    };
    if url.host_str() != Some("sso.rtl.be") {
        return;
    }

    if let Some(callback) = url.fragment().and_then(sso_oidc_callback_from_fragment) {
        let login_return_url = sso_login_return_url(&callback);
        push_unique(urls, callback);
        push_unique(urls, login_return_url);
        return;
    }

    if url.path() == "/" && url.query().is_none() {
        return;
    }

    push_unique(urls, url_path_with_query(&url));
}

pub(super) fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

pub(super) fn url_path_with_query(url: &Url) -> String {
    match url.query() {
        Some(query) if !query.is_empty() => format!("{}?{}", url.path(), query),
        _ => url.path().to_string(),
    }
}

pub(super) fn sso_oidc_callback_from_fragment(fragment: &str) -> Option<String> {
    let query = fragment.split_once('?')?.1;
    let params = serde_urlencoded::from_str::<HashMap<String, String>>(query).ok()?;
    let callback = params.get("oidc_callback")?.trim();
    if callback.is_empty() {
        return None;
    }

    Some(normalize_sso_redirect_path(callback))
}

pub(super) fn sso_login_return_url(oidc_callback: &str) -> String {
    format!(
        "/oidc/account/login?ReturnUrl={}",
        encode_query_value(oidc_callback)
    )
}

pub(super) fn sso_authorize_url_from_redirect(value: &str) -> Option<String> {
    let redirect_path = normalize_sso_redirect_path(value);
    let authorize_path = if redirect_path.starts_with("/oidc/connect/authorize") {
        Some(redirect_path)
    } else if redirect_path.starts_with("/oidc/account/login") {
        sso_return_url_from_login_path(&redirect_path)
    } else {
        None
    }?;

    Some(format!(
        "{}{}",
        SSO_BASE_URL.trim_end_matches('/'),
        authorize_path
    ))
}

pub(super) fn sso_return_url_from_login_path(value: &str) -> Option<String> {
    let url = Url::parse(&format!("{}{}", SSO_BASE_URL.trim_end_matches('/'), value)).ok()?;
    if url.host_str() != Some("sso.rtl.be") || url.path() != "/oidc/account/login" {
        return None;
    }

    url.query_pairs()
        .find(|(name, _)| name.eq_ignore_ascii_case("ReturnUrl"))
        .map(|(_, value)| normalize_sso_redirect_path(&value))
        .filter(|value| value.starts_with("/oidc/connect/authorize"))
}

pub(super) fn normalize_sso_redirect_path(value: &str) -> String {
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

pub(super) fn encode_query_value(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(*byte as char)
            }
            _ => encoded.push_str(&format!("%{:02X}", byte)),
        }
    }
    encoded
}

pub(super) fn rtlplay_player_cookies(cookies: &HashMap<String, String>) -> HashMap<String, String> {
    cookies
        .iter()
        .filter(|(name, _)| {
            matches!(
                name.as_str(),
                "lfvp_device_id"
                    | "lfvp_rtlplay_auth"
                    | "lfvp_access_token"
                    | "lfvp_auth.redirect_uri"
            )
        })
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}

pub(super) fn cookie_name_summary(cookies: &HashMap<String, String>) -> String {
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

pub(super) fn header_map_to_hash_map(headers: &HeaderMap) -> HashMap<String, String> {
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

pub(super) fn generic_hash_headers() -> HashMap<String, String> {
    HashMap::from([
        ("accept".to_string(), "*/*".to_string()),
        (
            "accept-language".to_string(),
            "fr,fr-FR;q=0.8,en-US;q=0.5,en;q=0.3".to_string(),
        ),
        (
            "user-agent".to_string(),
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:128.0) Gecko/20100101 Firefox/128.0"
                .to_string(),
        ),
        ("connection".to_string(), "keep-alive".to_string()),
        ("upgrade-insecure-requests".to_string(), "1".to_string()),
        ("sec-fetch-dest".to_string(), "document".to_string()),
        ("sec-fetch-mode".to_string(), "navigate".to_string()),
        ("sec-fetch-site".to_string(), "none".to_string()),
        ("sec-fetch-user".to_string(), "?1".to_string()),
        ("sec-gpc".to_string(), "1".to_string()),
        ("priority".to_string(), "u=0, i".to_string()),
    ])
}

pub(super) fn sso_hash_headers() -> HashMap<String, String> {
    let mut headers = generic_hash_headers();
    headers.insert("referer".to_string(), SSO_BASE_URL.to_string());
    headers.insert("sec-fetch-site".to_string(), "same-origin".to_string());
    headers
}

pub(super) fn load_credentials(
    credentials_store: &dyn CredentialsStore,
) -> Result<(String, String)> {
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

pub(super) fn load_cached_session(login: &str) -> Option<RtlPlaySession> {
    let cache = RTLPLAY_SESSION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache_guard) = cache.lock() else {
        return None;
    };

    let now = Instant::now();
    cache_guard.retain(|_, entry| entry.expires_at > now);
    cache_guard.get(login).map(|entry| entry.session.clone())
}

pub(super) fn save_cached_session(login: &str, session: RtlPlaySession, expires_at: Instant) {
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

pub(super) fn jwt_expires_at(token: &str) -> Option<Instant> {
    let payload = token.split('.').nth(1)?;
    let decoded = general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| general_purpose::URL_SAFE.decode(payload))
        .ok()?;
    let payload: Value = serde_json::from_slice(&decoded).ok()?;
    let exp = payload.get("exp")?.as_u64()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();

    if exp <= now + RTLPLAY_JWT_REFRESH_MARGIN_SECONDS {
        return None;
    }

    Some(Instant::now() + Duration::from_secs(exp - now - RTLPLAY_JWT_REFRESH_MARGIN_SECONDS))
}
