//! Reusable browser navigation, page actions and in-page fetch execution.

use super::*;

impl ArachneaHttpClient {
    pub(super) async fn open_browser_page(
        &self,
        engine: &DynHttpEngine,
    ) -> Result<Box<dyn crate::browser::BrowserPageSession>, ArachneaHttpError> {
        Ok(Box::new(RuntimeBoundBrowserPage {
            page: engine.open_browser_page_session().await?,
            _runtime: self._proxy_runtime.clone(),
        }))
    }

    /// Returns a reference to the browser session manager, if available.
    pub fn browser_session_manager(&self) -> Option<&Arc<BrowserSessionManager>> {
        self.browser_session_manager.as_ref()
    }

    /// Closes the retained browser session for the origin of one URL.
    ///
    /// # Errors
    ///
    /// Returns `InvalidUrl` when `url` does not contain a valid origin.
    pub fn close_browser_session(&self, url: &str) -> Result<(), ArachneaHttpError> {
        let origin = origin_url(url)?;
        if let Some(manager) = &self.browser_session_manager {
            manager.invalidate_origin(&origin);
        }
        Ok(())
    }

    /// Navigates a reusable browser page and returns its stable HTML when requested.
    ///
    /// The page is scoped to the navigation origin, configured user-agent profile,
    /// and proxy route. Browser cookies and the observed user-agent are handed back
    /// to the existing shared HTTP state after navigation.
    ///
    /// # Errors
    ///
    /// Returns `UnsupportedEngineOperation` when the configured browser solver does
    /// not implement persistent pages, or browser/session failures otherwise.
    pub async fn page_navigate(
        &self,
        navigation: PageNavigationRequest,
    ) -> Result<PageNavigationResponse, ArachneaHttpError> {
        let origin = origin_url(&navigation.url)?;
        let manager = self.browser_session_manager.as_ref().ok_or_else(|| {
            ArachneaHttpError::BrowserSessionUnavailable {
                origin: origin.clone(),
                reason: "browser session management is disabled".to_string(),
            }
        })?;
        let session_key = BrowserSessionKey {
            origin: origin.clone(),
            profile: self.config.user_agent_profile.user_agent().to_string(),
            proxy_route: self
                ._proxy_runtime
                .proxy_url(&self.config)
                .map(str::to_owned),
        };
        let handle = manager.get_or_create(&session_key);
        if !handle.is_valid() {
            return Err(ArachneaHttpError::BrowserSessionUnavailable {
                origin,
                reason: "the cached browser session was invalidated".to_string(),
            });
        }

        let result = async {
            let mut session = handle.lock().await;
            if session.page.is_none() {
                let engine = self
                    .browser_cloudflare_engine
                    .as_ref()
                    .ok_or_else(|| ArachneaHttpError::CloudflareSolverUnavailable)?;
                session.page = Some(self.open_browser_page(engine).await?);
            }
            let page = session.page.as_mut().expect("browser page initialized");
            let response = page.navigate(navigation).await?;
            self.store_browser_session_metadata(page.metadata().await?)
                .await?;
            handle.touch();
            Ok(response)
        }
        .await;

        if result.is_err() {
            manager.invalidate_origin(&origin);
        }
        result
    }

    /// Navigates a reusable browser page, clicks an element, and waits for a result selector.
    ///
    /// The page is scoped to the navigation origin, configured user-agent profile,
    /// and proxy route. Browser cookies and the observed user-agent are handed back
    /// to the existing shared HTTP state after navigation and interaction.
    ///
    /// # Errors
    ///
    /// Returns `UnsupportedEngineOperation` when the configured browser solver does
    /// not support page interaction, or browser/session failures otherwise.
    pub async fn page_click(
        &self,
        navigation: PageNavigationRequest,
        click: PageClickRequest,
    ) -> Result<PageClickResponse, ArachneaHttpError> {
        let origin = origin_url(&navigation.url)?;
        let manager = self.browser_session_manager.as_ref().ok_or_else(|| {
            ArachneaHttpError::BrowserSessionUnavailable {
                origin: origin.clone(),
                reason: "browser session management is disabled".to_string(),
            }
        })?;
        let session_key = BrowserSessionKey {
            origin: origin.clone(),
            profile: self.config.user_agent_profile.user_agent().to_string(),
            proxy_route: self
                ._proxy_runtime
                .proxy_url(&self.config)
                .map(str::to_owned),
        };
        let handle = manager.get_or_create(&session_key);
        if !handle.is_valid() {
            return Err(ArachneaHttpError::BrowserSessionUnavailable {
                origin,
                reason: "the cached browser session was invalidated".to_string(),
            });
        }

        let result = async {
            let mut session = handle.lock().await;
            if session.page.is_none() {
                let engine = self
                    .browser_cloudflare_engine
                    .as_ref()
                    .ok_or_else(|| ArachneaHttpError::CloudflareSolverUnavailable)?;
                session.page = Some(self.open_browser_page(engine).await?);
            }
            let page = session.page.as_mut().expect("browser page initialized");
            page.navigate(navigation).await?;
            self.store_browser_session_metadata(page.metadata().await?)
                .await?;
            let response = page.click_and_wait(click).await?;
            self.store_browser_session_metadata(page.metadata().await?)
                .await?;
            handle.touch();
            Ok(response)
        }
        .await;

        if result.is_err() {
            manager.invalidate_origin(&origin);
        }
        result
    }

    /// Opens a dedicated browser page, clicks an element, and closes the page.
    ///
    /// This is intended for independent interactions that may run concurrently.
    /// It deliberately does not enter the origin-scoped retained-session cache.
    pub async fn page_click_isolated(
        &self,
        navigation: PageNavigationRequest,
        click: PageClickRequest,
    ) -> Result<PageClickResponse, ArachneaHttpError> {
        let engine = self
            .browser_cloudflare_engine
            .as_ref()
            .ok_or_else(|| ArachneaHttpError::CloudflareSolverUnavailable)?;
        let mut page = self.open_browser_page(engine).await?;
        let result = async {
            page.navigate(navigation).await?;
            self.store_browser_session_metadata(page.metadata().await?)
                .await?;
            let response = page.click_and_wait(click).await?;
            self.store_browser_session_metadata(page.metadata().await?)
                .await?;
            Ok(response)
        }
        .await;
        page.close().await;
        result
    }

    /// Navigates a reusable browser page, then executes one JavaScript fetch in it.
    ///
    /// The page is scoped to the navigation origin, configured user-agent
    /// profile, and proxy route. Browser cookies and the observed user-agent
    /// are handed back to the existing shared HTTP state after both navigation
    /// and fetch.
    ///
    /// # Errors
    ///
    /// Returns `UnsupportedEngineOperation` when the configured browser solver
    /// does not implement persistent pages, `TokenAbsent` when a requested
    /// callback token is unavailable, or browser/session failures otherwise.
    pub async fn page_fetch(
        &self,
        navigation: PageNavigationRequest,
        request: PageFetchRequest,
    ) -> Result<PageFetchResponse, ArachneaHttpError> {
        #[cfg(feature = "arachnea-proxy")]
        if self.config.proxy_response_classifier.is_some() {
            tracing::warn!(target: "arachnea_http::client", "browser page fetch does not support correlated early proxy response classification; no preference or exclusion will be inferred");
        }
        let origin = origin_url(&navigation.url)?;
        let manager = self.browser_session_manager.as_ref().ok_or_else(|| {
            ArachneaHttpError::BrowserSessionUnavailable {
                origin: origin.clone(),
                reason: "browser session management is disabled".to_string(),
            }
        })?;
        let session_key = BrowserSessionKey {
            origin: origin.clone(),
            profile: self.config.user_agent_profile.user_agent().to_string(),
            proxy_route: self
                ._proxy_runtime
                .proxy_url(&self.config)
                .map(str::to_owned),
        };
        let handle = manager.get_or_create(&session_key);
        if !handle.is_valid() {
            return Err(ArachneaHttpError::BrowserSessionUnavailable {
                origin,
                reason: "the cached browser session was invalidated".to_string(),
            });
        }

        let result = async {
            let mut session = handle.lock().await;
            if session.page.is_none() {
                let engine = self
                    .browser_cloudflare_engine
                    .as_ref()
                    .ok_or_else(|| ArachneaHttpError::CloudflareSolverUnavailable)?;
                session.page = Some(self.open_browser_page(engine).await?);
            }
            let page = session.page.as_mut().expect("browser page initialized");
            page.navigate(navigation).await?;
            self.store_browser_session_metadata(page.metadata().await?)
                .await?;

            let token_submitted = request.turnstile_token_placeholder.is_some();
            let request = if token_submitted {
                let token = if request.reuse_turnstile_token {
                    if let Some(token) = handle.cached_turnstile_token() {
                        token
                    } else {
                        let token = page.read_turnstile_token().await?.ok_or_else(|| {
                            ArachneaHttpError::TokenAbsent {
                                origin: origin.clone(),
                            }
                        })?;
                        handle.cache_turnstile_token(token.clone());
                        token
                    }
                } else {
                    page.read_turnstile_token().await?.ok_or_else(|| {
                        ArachneaHttpError::TokenAbsent {
                            origin: origin.clone(),
                        }
                    })?
                };
                request.inject_turnstile_token(&token)?
            } else {
                request
            };
            let token_rejection = request.clone();
            let response = page.fetch(request).await?;
            self.store_browser_session_metadata(page.metadata().await?)
                .await?;
            if token_submitted && token_rejection.is_token_rejected_by(&response) {
                page.clear_turnstile_token().await?;
                handle.clear_turnstile_token();
                return Err(ArachneaHttpError::TokenRejected {
                    origin: origin.clone(),
                });
            }
            handle.touch();
            Ok(response)
        }
        .await;

        if !matches!(result, Err(ArachneaHttpError::TokenRejected { .. })) && result.is_err() {
            manager.invalidate_origin(&origin);
        }
        result
    }
}

#[cfg(test)]
#[path = "../../tests/client/browser_sessions.rs"]
mod tests;
