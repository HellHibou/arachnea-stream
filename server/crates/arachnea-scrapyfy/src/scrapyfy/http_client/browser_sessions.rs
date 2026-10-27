//! Browser sessions responsibilities.

use super::*;

impl HttpClient {
    /// Navigates a browser page and returns its stable HTML for a scraper query.
    ///
    /// The browser session and cookie handoff are provided by `arachnea-http`.
    pub async fn page_navigate_for_request(
        &self,
        url: &str,
        request_headers: &HashMap<String, String>,
    ) -> Result<String> {
        let headers = header_map_from_strings(
            request_headers
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str())),
        )?;
        let response = self
            .http_client()
            .await?
            .page_navigate(PageNavigationRequest {
                url: url.to_string(),
                headers,
                collect_body: true,
            })
            .await?;
        response
            .body
            .ok_or_else(|| anyhow::anyhow!("browser navigation did not return page HTML"))
    }

    /// Navigates a browser page, clicks one element, and returns the rendered HTML.
    pub async fn page_click_for_request(
        &self,
        page_url: &str,
        selector: &str,
        wait_for_selector: &str,
    ) -> Result<String> {
        let response = self
            .http_client()
            .await?
            .page_click(
                PageNavigationRequest {
                    url: page_url.to_string(),
                    headers: HeaderMap::new(),
                    collect_body: false,
                },
                PageClickRequest {
                    selector: selector.to_string(),
                    wait_for_selector: wait_for_selector.to_string(),
                },
            )
            .await?;
        Ok(response.body)
    }

    /// Opens an isolated browser page, clicks an element, and returns the rendered HTML.
    ///
    /// Unlike [`Self::page_click_for_request`], this operation does not reuse the
    /// origin-scoped page session, so independent clicks can run concurrently.
    pub async fn page_click_isolated_for_request(
        &self,
        page_url: &str,
        selector: &str,
        wait_for_selector: &str,
    ) -> Result<String> {
        let response = self
            .http_client()
            .await?
            .page_click_isolated(
                PageNavigationRequest {
                    url: page_url.to_string(),
                    headers: HeaderMap::new(),
                    collect_body: false,
                },
                PageClickRequest {
                    selector: selector.to_string(),
                    wait_for_selector: wait_for_selector.to_string(),
                },
            )
            .await?;
        Ok(response.body)
    }

    /// Closes the retained browser session associated with one page URL.
    pub async fn close_browser_session_for_url(&self, url: &str) -> Result<()> {
        self.http_client().await?.close_browser_session(url)?;
        Ok(())
    }

    /// Navigates a browser page and executes one JavaScript fetch for a scraper sub-query.
    ///
    /// The browser session, cookie handoff, callback token capture, and token
    /// rejection classification are provided by `arachnea-http`.
    pub async fn page_fetch_for_request(
        &self,
        page_url: &str,
        method: Method,
        url: &str,
        request_headers: &HashMap<String, String>,
        request_body: Option<&str>,
        token: Option<&ScraperBrowserTokenConfig>,
    ) -> Result<String> {
        let headers = header_map_from_strings(
            request_headers
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str())),
        )?;
        let token_rejection_statuses = token
            .map(|config| {
                config
                    .rejection_statuses
                    .iter()
                    .filter_map(|status| http::StatusCode::from_u16(*status).ok())
                    .collect()
            })
            .unwrap_or_default();
        let response = self
            .http_client()
            .await?
            .page_fetch(
                PageNavigationRequest {
                    url: page_url.to_string(),
                    headers: HeaderMap::new(),
                    collect_body: false,
                },
                PageFetchRequest {
                    method,
                    url: url.to_string(),
                    headers,
                    body: request_body.map(|body| body.as_bytes().to_vec().into()),
                    turnstile_token_placeholder: token.map(|config| config.placeholder.clone()),
                    reuse_turnstile_token: token.is_some_and(|config| {
                        config.cache_scope == Some(ScraperBrowserTokenCacheScope::Domain)
                    }),
                    token_rejection_statuses,
                    token_rejection_body_markers: token
                        .map(|config| config.rejection_body_markers.clone())
                        .unwrap_or_default(),
                },
            )
            .await?;
        Ok(response.body)
    }
}
