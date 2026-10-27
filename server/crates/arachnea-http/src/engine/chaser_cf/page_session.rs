//! Retained chaser browser pages and in-page operations.

use super::*;

/// Interactive chaser-cf page session on the process-wide shared browser.
///
/// The page is retained for the session lifetime, which is what makes the
/// automatic Cloudflare flow possible in one window:
///
/// 1. `navigate` loads the target page.
/// 2. `click_and_wait` clicks the button that dynamically injects the
///    Cloudflare captcha and solves it automatically.
/// 3. `read_turnstile_token` polls for the resulting token.
/// 4. Once resolved, `metadata` exposes the resulting cookies and user-agent
///    and the captcha-driven iframe can be read through `fetch`.
pub(super) struct ChaserCfPageSession {
    /// Retained CDP page handle.
    pub(super) page: Page,
    /// Retained JavaScript evaluation helper over `page`.
    pub(super) chaser: ChaserPage,
}

#[async_trait]
impl BrowserPageSession for ChaserCfPageSession {
    async fn navigate(
        &mut self,
        request: PageNavigationRequest,
    ) -> Result<PageNavigationResponse, ArachneaHttpError> {
        self.chaser
            .goto(&request.url)
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
        let url = self.page.url().await.ok().flatten().unwrap_or(request.url);
        let body = if request.collect_body {
            let html = self
                .chaser
                .content()
                .await
                .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
            Some(html)
        } else {
            None
        };
        Ok(PageNavigationResponse { url, body })
    }

    async fn fetch(
        &mut self,
        request: PageFetchRequest,
    ) -> Result<PageFetchResponse, ArachneaHttpError> {
        let mut headers = HashMap::new();
        for (name, value) in &request.headers {
            let value = value
                .to_str()
                .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?;
            headers.insert(name.as_str().to_string(), value.to_string());
        }
        let body = request
            .body
            .map(|body| {
                String::from_utf8(body.to_vec()).map_err(|_| {
                    ArachneaHttpError::PageFetchFailed(
                        "in-page fetch currently requires a UTF-8 request body".to_string(),
                    )
                })
            })
            .transpose()?;
        let payload = serde_json::json!({
            "method": request.method.as_str(),
            "url": request.url,
            "headers": headers,
            "body": body,
        });
        let script = format!(
            include_str!("../../../resources/chaser_cf/page_fetch.js"),
            serde_json::to_string(&payload)
                .map_err(|err| ArachneaHttpError::PageFetchFailed(err.to_string()))?
        );
        let result = self
            .page
            .evaluate_function(script)
            .await
            .map_err(|err| ArachneaHttpError::PageFetchFailed(err.to_string()))?
            .into_value::<InPageFetchResult>()
            .map_err(|err| ArachneaHttpError::PageFetchFailed(err.to_string()))?;
        let mut response_headers = HeaderMap::new();
        for (name, value) in result.headers {
            let name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|err| ArachneaHttpError::PageFetchFailed(err.to_string()))?;
            let value = HeaderValue::from_str(&value)
                .map_err(|err| ArachneaHttpError::PageFetchFailed(err.to_string()))?;
            response_headers.append(name, value);
        }
        Ok(PageFetchResponse {
            url: result.url,
            status: StatusCode::from_u16(result.status).map_err(|err| {
                ArachneaHttpError::PageFetchFailed(format!("invalid browser fetch status: {err}"))
            })?,
            headers: response_headers,
            body: result.body,
        })
    }

    async fn click_and_wait(
        &mut self,
        request: PageClickRequest,
    ) -> Result<PageClickResponse, ArachneaHttpError> {
        let selector = serde_json::to_string(&request.selector)
            .map_err(|err| ArachneaHttpError::PageInteractionFailed(err.to_string()))?;
        let click_script = format!(
            include_str!("../../../resources/chaser_cf/click_target.js"),
            selector = selector
        );
        self.chaser
            .evaluate(&click_script)
            .await
            .map_err(|err| ArachneaHttpError::PageInteractionFailed(err.to_string()))?;
        debug_dump(&self.chaser, "after-click").await;

        let wait_script = format!(
            include_str!("../../../resources/chaser_cf/wait_for_selector.js"),
            selector = serde_json::to_string(&request.wait_for_selector)
                .map_err(|err| ArachneaHttpError::PageInteractionFailed(err.to_string()))?
        );
        let started = Instant::now();
        let mut deadline = started + CLICK_WAIT_TIMEOUT;
        let mut solve_started = false;
        let mut last_front = started - crate::HTTP_CHASER_INITIAL_FRONT_OFFSET;
        loop {
            // If a popup still manages to open, the solver tab falls in the
            // background and its timers get throttled, stalling the captcha.
            // Re-activating the tab periodically counteracts that.
            if last_front.elapsed() >= crate::HTTP_CHASER_FOREGROUND_POLL_INTERVAL {
                self.page.bring_to_front().await.ok();
                last_front = Instant::now();
            }
            if !solve_started && challenge_is_present(&self.chaser).await {
                // The clicked button injected a Cloudflare captcha into the
                // page. Solve it automatically (wait for `cf_clearance` or a
                // Turnstile token and click the challenge) instead of relying
                // on a manual solve. The deadline is extended because the
                // target selector only appears once the captcha is solved.
                solve_started = true;
                deadline = started + CLICK_SOLVE_TIMEOUT + CLICK_WAIT_TIMEOUT;
                info!(target: "arachnea_http::engine::chaser_cf",
                    selector = %request.wait_for_selector,
                    "chaser-cf: captcha detected after page action click, solving"
                );
                wait_for_injected_captcha(&self.page, &self.chaser, CLICK_SOLVE_TIMEOUT).await;
                info!(target: "arachnea_http::engine::chaser_cf", "chaser-cf: captcha solve finished");
            }

            let found = self
                .chaser
                .evaluate(&wait_script)
                .await
                .map_err(|err| ArachneaHttpError::PageInteractionFailed(err.to_string()))?
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            if found {
                let body = self
                    .chaser
                    .content()
                    .await
                    .map_err(|err| ArachneaHttpError::PageInteractionFailed(err.to_string()))?;
                info!(target: "arachnea_http::engine::chaser_cf", "chaser-cf: page action result ready ({} bytes)", body.len());
                return Ok(PageClickResponse { body });
            }
            if Instant::now() >= deadline {
                warn!(target: "arachnea_http::engine::chaser_cf",
                    selector = %request.wait_for_selector,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "chaser-cf: timed out waiting for the page action result"
                );
                return Err(ArachneaHttpError::PageInteractionFailed(
                    "timed out waiting for the page action result".to_string(),
                ));
            }
            tokio::time::sleep(crate::HTTP_CHASER_PAGE_SELECTOR_POLL_INTERVAL).await;
        }
    }

    async fn metadata(&mut self) -> Result<BrowserSessionMetadata, ArachneaHttpError> {
        let cookies = self
            .page
            .get_cookies()
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
        let mut headers = HeaderMap::new();
        for cookie in cookies {
            let chaser_cookie = ChaserCookie {
                name: cookie.name,
                value: cookie.value,
                domain: Some(cookie.domain),
                path: Some(cookie.path),
                expires: Some(cookie.expires),
                http_only: Some(cookie.http_only),
                secure: Some(cookie.secure),
                same_site: cookie.same_site.map(|value| format!("{value:?}")),
            };
            headers.append(
                SET_COOKIE,
                HeaderValue::from_str(&set_cookie_header(&chaser_cookie))
                    .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?,
            );
        }
        if let Some(user_agent) = self
            .chaser
            .evaluate("navigator.userAgent")
            .await
            .ok()
            .and_then(|value| value?.as_str().map(str::to_owned))
            .filter(|value| !value.is_empty())
        {
            headers.insert(
                SOLVER_USER_AGENT_HEADER,
                HeaderValue::from_str(&user_agent)
                    .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?,
            );
        }
        let url = self
            .page
            .url()
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?
            .unwrap_or_default();
        Ok(BrowserSessionMetadata { url, headers })
    }

    async fn read_turnstile_token(&mut self) -> Result<Option<String>, ArachneaHttpError> {
        let script = crate::HTTP_CHASER_SCRIPT_READ_TURNSTILE_TOKEN;
        let started = Instant::now();
        loop {
            let token = self
                .chaser
                .evaluate(script)
                .await
                .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?
                .and_then(|value| value.as_str().map(str::to_owned));
            if let Some(token) = token {
                return Ok(Some(token));
            }
            if started.elapsed() >= TURNSTILE_WAIT_TIMEOUT {
                return Ok(None);
            }
            tokio::time::sleep(crate::HTTP_CHASER_TOKEN_POLL_INTERVAL).await;
        }
    }

    async fn clear_turnstile_token(&mut self) -> Result<(), ArachneaHttpError> {
        let script = crate::HTTP_CHASER_SCRIPT_CLEAR_TURNSTILE_TOKEN;
        self.chaser
            .evaluate(script)
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(err.to_string()))?;
        Ok(())
    }

    async fn close(self: Box<Self>) {
        // The browser manager is shared process-wide and stays alive; dropping
        // the page alone does not close its Chrome tab.
        let ChaserCfPageSession { page, chaser: _ } = *self;
        if let Err(err) = page.close().await {
            warn!(target: "arachnea_http::engine::chaser_cf", error = %err, "failed to close chaser-cf browser session page");
        }
    }
}
