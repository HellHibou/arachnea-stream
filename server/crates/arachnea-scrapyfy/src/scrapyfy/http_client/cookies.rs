//! Cookie-cache access for scraper clients.

use super::*;

impl HttpClient {
    /// Stores simple name/value cookies for a URL in the shared HTTP cache.
    pub async fn store_cookies_for_url(url: &str, cookies: &HashMap<String, String>) -> Result<()> {
        Self::store_cookies_in_cache(global_cookie_cache(), url, cookies).await
    }

    /// Stores simple name/value cookies for a URL in this client's cookie cache.
    pub async fn store_cookies_for_url_in_client(
        &self,
        url: &str,
        cookies: &HashMap<String, String>,
    ) -> Result<()> {
        Self::store_cookies_in_cache(self.cookie_cache.clone(), url, cookies).await
    }

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
    pub async fn cookies_for_url_in_client(&self, url: &str) -> Result<HashMap<String, String>> {
        Self::cookies_for_url_in_cache(self.cookie_cache.clone(), url).await
    }

    pub(super) async fn cookies_for_url_in_cache(
        cache: Arc<AsyncRwLock<SharedCookieCache>>,
        url: &str,
    ) -> Result<HashMap<String, String>> {
        let mut cache = cache.write().await;
        let cookies = cache.cookies_for_url(url)?;
        Ok(cookies)
    }
}
