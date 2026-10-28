//! Cookie-cache access for scraper clients.

use super::*;

impl HttpClient {
    /// Stores simple name/value cookies for a URL in the shared HTTP cache.
    ///
    /// # Arguments
    ///
    /// * `url` - Absolute request URL used by the HTTP transport.
    /// * `cookies` - Simple cookie names and values; blank names or values are skipped after trimming.
    ///
    /// # Errors
    ///
    /// Returns an error if the URL is invalid or has no host.
    pub async fn store_cookies_for_url(url: &str, cookies: &HashMap<String, String>) -> Result<()> {
        Self::store_cookies_in_cache(global_cookie_cache(), url, cookies).await
    }

    /// Stores simple name/value cookies for a URL in this client's cookie cache.
    ///
    /// # Arguments
    ///
    /// * `url` - Absolute request URL used by the HTTP transport.
    /// * `cookies` - Simple cookie names and values; blank names or values are skipped after trimming.
    ///
    /// # Errors
    ///
    /// Returns an error if the URL is invalid or has no host.
    pub async fn store_cookies_for_url_in_client(
        &self,
        url: &str,
        cookies: &HashMap<String, String>,
    ) -> Result<()> {
        Self::store_cookies_in_cache(self.cookie_cache.clone(), url, cookies).await
    }

    /// Stores trimmed non-empty cookies using the URL host, path and HTTPS security flag.
    ///
    /// # Arguments
    ///
    /// * `cache` - Cookie cache from which matching cookies are read or into which they are inserted.
    /// * `url` - Absolute request URL used by the HTTP transport.
    /// * `cookies` - Simple cookie names and values; blank names or values are skipped after trimming.
    ///
    /// # Errors
    ///
    /// Returns an error if the URL is invalid or has no host; blank cookie pairs are skipped.
    pub(super) async fn store_cookies_in_cache(
        cache: Arc<AsyncRwLock<SharedCookieCache>>,
        url: &str,
        cookies: &HashMap<String, String>,
    ) -> Result<()> {
        let url = Url::parse(url)?;
        let domain = url
            .host_str()
            .context("Cookie URL must contain a host")?
            .to_string();
        let path = if url.path().is_empty() {
            "/".to_string()
        } else {
            url.path().to_string()
        };
        let secure = url.scheme() == "https";
        let mut cache = cache.write().await;

        for (name, value) in cookies {
            let name = name.trim();
            let value = value.trim();
            if name.is_empty() || value.is_empty() {
                continue;
            }

            cache.insert_cookie(CookieEntry {
                name: name.to_string(),
                value: value.to_string(),
                domain: domain.clone(),
                path: path.clone(),
                expires_at: None,
                secure,
                http_only: false,
                same_site: None,
            });
        }

        Ok(())
    }

    /// Returns the shared-cookie view currently applicable to one absolute URL.
    ///
    /// # Arguments
    ///
    /// * `url` - Absolute URL whose matching cookies should be returned.
    ///
    /// # Errors
    ///
    /// Returns an error if `url` is invalid.
    pub async fn cookies_for_url(url: &str) -> Result<HashMap<String, String>> {
        Self::cookies_for_url_in_cache(global_cookie_cache(), url).await
    }

    /// Returns this client's cookie view currently applicable to one absolute URL.
    ///
    /// # Arguments
    ///
    /// * `url` - Absolute request URL used by the HTTP transport.
    pub async fn cookies_for_url_in_client(&self, url: &str) -> Result<HashMap<String, String>> {
        Self::cookies_for_url_in_cache(self.cookie_cache.clone(), url).await
    }

    /// Returns the cache view applicable to the URL while allowing expired-cookie cleanup.
    ///
    /// # Arguments
    ///
    /// * `cache` - Cookie cache from which matching cookies are read or into which they are inserted.
    /// * `url` - Absolute request URL used by the HTTP transport.
    ///
    /// # Errors
    ///
    /// Returns an error if the cookie cache cannot parse the supplied URL.
    pub(super) async fn cookies_for_url_in_cache(
        cache: Arc<AsyncRwLock<SharedCookieCache>>,
        url: &str,
    ) -> Result<HashMap<String, String>> {
        let mut cache = cache.write().await;
        let cookies = cache.cookies_for_url(url)?;
        Ok(cookies)
    }
}
