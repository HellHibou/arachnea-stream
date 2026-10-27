//! Serializable HTTP and browser request configuration.

use super::*;

/// YAML-selectable request mode for scraper HTTP calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScraperHttpMode {
    /// Automatically select the best request mode based on the target.
    Auto,
    /// Use direct HTTP requests without any special handling.
    Direct,
    /// Use Cloudflare smart mode for handling Cloudflare-protected sites.
    CloudflareSmart,
    /// Use browser-like requests for Cloudflare-protected sites.
    CloudflareBrowser,
}

/// Execution transport for a scraper query or sub-query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScraperHttpExecution {
    /// Use the normal HTTP client request path.
    Direct,
    /// Navigate a retained browser page and return its stable HTML source.
    PageNavigate,
    /// Navigate a retained browser page, click an element, then return rendered HTML.
    PageClick,
    /// Navigate a retained browser page, then execute JavaScript `fetch()` in it.
    PageFetch,
}

/// Scope used to reuse browser page sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScraperBrowserContext {
    /// Reuse a session only when origin, browser profile, and proxy route match.
    Origin,
    /// Open a dedicated page for one `page_click` sub-query execution.
    Isolated,
}

/// Browser-side source from which one request token is captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScraperBrowserTokenSource {
    /// Application token passed to `turnstile.render(... callback(token))`.
    TurnstileCallback,
}

/// Maximum browser-token retry policy for one sub-query execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScraperBrowserTokenRetry {
    /// Never retry after a classified token rejection.
    Never,
    /// Re-navigate once, obtain a fresh token, and submit one more request.
    Once,
}

/// Optional reuse scope for a browser callback token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScraperBrowserTokenCacheScope {
    /// Reuse the token only in the matching origin/profile/proxy browser session.
    Domain,
}

/// Token substitution and rejection signals for a browser page fetch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScraperBrowserTokenConfig {
    /// Source used to capture the token in the browser page.
    pub source: ScraperBrowserTokenSource,
    /// Literal request-body placeholder replaced with the captured token.
    pub placeholder: String,
    /// Optional opt-in reuse scope for the captured token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_scope: Option<ScraperBrowserTokenCacheScope>,
    /// Bounded retry policy when the target rejects the captured token.
    #[serde(default = "default_browser_token_retry")]
    pub retry_on_rejection: ScraperBrowserTokenRetry,
    /// HTTP status codes classified as token rejection after token submission.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejection_statuses: Vec<u16>,
    /// Response text fragments classified as token rejection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejection_body_markers: Vec<String>,
}

pub(super) fn default_browser_token_retry() -> ScraperBrowserTokenRetry {
    crate::BROWSER_DEFAULT_TOKEN_RETRY
}

/// Browser-page click and DOM wait configuration for a scraper sub-query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScraperBrowserClickConfig {
    /// CSS selector for the page element that triggers the site workflow.
    pub selector: String,
    /// CSS selector that must appear before the rendered page is extracted.
    pub wait_for_selector: String,
}

impl From<ScraperHttpMode> for HttpRequestMode {
    /// Converts the scraper-level mode into the [`HttpRequestMode`] expected by `arachnea-http`.
    fn from(mode: ScraperHttpMode) -> Self {
        match mode {
            ScraperHttpMode::Auto => Self::Auto,
            ScraperHttpMode::Direct => Self::Direct,
            ScraperHttpMode::CloudflareSmart => Self::CloudflareSmart,
            ScraperHttpMode::CloudflareBrowser => Self::CloudflareBrowser,
        }
    }
}

/// YAML-selectable browser profile used by `arachnea-http`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScraperHttpUserAgentProfile {
    /// Use Chrome browser profile.
    Chrome,
    /// Use stable Chrome browser profile.
    ChromeStable,
    /// Use Firefox browser profile.
    Firefox,
    /// Use stable Firefox browser profile.
    FirefoxStable,
}

impl From<ScraperHttpUserAgentProfile> for BrowserProfile {
    /// Converts the scraper-level profile into the [`BrowserProfile`] expected by `arachnea-http`.
    fn from(profile: ScraperHttpUserAgentProfile) -> Self {
        match profile {
            ScraperHttpUserAgentProfile::Chrome | ScraperHttpUserAgentProfile::ChromeStable => {
                Self::ChromeStable
            }
            ScraperHttpUserAgentProfile::Firefox | ScraperHttpUserAgentProfile::FirefoxStable => {
                Self::FirefoxStable
            }
        }
    }
}

/// Scraper YAML HTTP options inherited from collection to query to sub-query.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScraperHttpConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// HTTP request mode override for this scraper call.
    pub mode: Option<ScraperHttpMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Browser profile used to generate the User-Agent header.
    pub user_agent_profile: Option<ScraperHttpUserAgentProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Custom User-Agent string override.
    pub user_agent: Option<String>,
    /// Ordered proxy country hints for geo-targeted requests.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proxy_countries: Vec<String>,
    /// Optional domains sharing an eligible default proxy, including their subdomains.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proxy_domain_group: Vec<String>,
    /// Opaque affinity key used to keep related requests on the same eligible proxy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_affinity: Option<String>,
    /// HTTP statuses that reject a destination association; retries obey replay policy.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proxy_rejection_statuses: Vec<u16>,
    /// Explicit operation-level replay authorization; requires a reproducible body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_retry_replayable: Option<bool>,
    /// Known response policy; overrides the legacy rejection list when present.
    #[cfg(feature = "arachnea-proxy")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_response_classifier:
        Option<arachnea_proxy::core::http::response_classifier::ProxyResponseClassifierConfig>,
    /// HTTP statuses that should produce an empty scraper response.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub empty_on_statuses: Vec<u16>,
    /// Maximum number of redirects to follow before returning an error.
    pub max_redirects: Option<usize>,
    /// Optional browser execution mode for a sub-query.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<ScraperHttpExecution>,
    /// Browser session reuse scope required by browser page execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_context: Option<ScraperBrowserContext>,
    /// Source page URL template navigated before an in-page fetch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_url: Option<String>,
    /// Optional browser token configuration for the in-page request body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_token: Option<ScraperBrowserTokenConfig>,
    /// Optional click and DOM wait configuration for `execution: page_click`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_click: Option<ScraperBrowserClickConfig>,
}

impl ScraperHttpConfig {
    /// Returns whether this configuration has no explicit options.
    pub fn is_empty(&self) -> bool {
        #[cfg(feature = "arachnea-proxy")]
        if self.proxy_response_classifier.is_some() {
            return false;
        }
        self.mode.is_none()
            && self.user_agent_profile.is_none()
            && self.user_agent.is_none()
            && self.proxy_countries.is_empty()
            && self.proxy_domain_group.is_empty()
            && self.proxy_affinity.is_none()
            && self.proxy_rejection_statuses.is_empty()
            && self.proxy_retry_replayable.is_none()
            && self.empty_on_statuses.is_empty()
            && self.max_redirects.is_none()
            && self.execution.is_none()
            && self.browser_context.is_none()
            && self.page_url.is_none()
            && self.browser_token.is_none()
            && self.browser_click.is_none()
    }

    /// Applies a child override on top of this config.
    pub fn merge(&self, child: &Self) -> Self {
        Self {
            proxy_retry_replayable: child.proxy_retry_replayable.or(self.proxy_retry_replayable),
            #[cfg(feature = "arachnea-proxy")]
            proxy_response_classifier: child
                .proxy_response_classifier
                .as_ref()
                .or(self.proxy_response_classifier.as_ref())
                .cloned(),
            mode: child.mode.or(self.mode),
            user_agent_profile: child.user_agent_profile.or(self.user_agent_profile),
            user_agent: child
                .user_agent
                .as_ref()
                .or(self.user_agent.as_ref())
                .cloned(),
            proxy_countries: if child.proxy_countries.is_empty() {
                self.proxy_countries.clone()
            } else {
                child.proxy_countries.clone()
            },
            proxy_domain_group: if child.proxy_domain_group.is_empty() {
                self.proxy_domain_group.clone()
            } else {
                child.proxy_domain_group.clone()
            },
            proxy_affinity: child
                .proxy_affinity
                .as_ref()
                .or(self.proxy_affinity.as_ref())
                .cloned(),
            proxy_rejection_statuses: if child.proxy_rejection_statuses.is_empty() {
                self.proxy_rejection_statuses.clone()
            } else {
                child.proxy_rejection_statuses.clone()
            },
            empty_on_statuses: if child.empty_on_statuses.is_empty() {
                self.empty_on_statuses.clone()
            } else {
                child.empty_on_statuses.clone()
            },
            max_redirects: child.max_redirects.or(self.max_redirects),
            execution: child.execution.or(self.execution),
            browser_context: child.browser_context.or(self.browser_context),
            page_url: child.page_url.as_ref().or(self.page_url.as_ref()).cloned(),
            browser_token: child
                .browser_token
                .as_ref()
                .or(self.browser_token.as_ref())
                .cloned(),
            browser_click: child
                .browser_click
                .as_ref()
                .or(self.browser_click.as_ref())
                .cloned(),
        }
    }

    /// Sets allowed proxy countries in canonical order after normalization and deduplication.
    pub fn proxy_countries(mut self, countries: impl IntoIterator<Item = String>) -> Self {
        self.proxy_countries = normalize_proxy_countries(countries);
        self
    }

    /// Sets an optional domain group passed to the generic proxy selector.
    ///
    /// # Arguments
    /// - `domains`: Domain names; matching includes their subdomains.
    pub fn proxy_domain_group(mut self, domains: impl IntoIterator<Item = String>) -> Self {
        self.proxy_domain_group = domains.into_iter().collect();
        self
    }

    /// Sets an opaque proxy affinity key after trimming it.
    pub fn proxy_affinity(mut self, affinity: impl Into<String>) -> Self {
        self.proxy_affinity = Some(affinity.into())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self
    }

    /// Returns the [`HttpRequestMode`] derived from this configuration.
    pub(super) fn request_mode(&self) -> HttpRequestMode {
        self.mode.unwrap_or(crate::SCRAPER_DEFAULT_HTTP_MODE).into()
    }

    /// Returns the [`BrowserProfile`] derived from user-agent or user-agent profile.
    pub(super) fn browser_profile(&self) -> BrowserProfile {
        self.user_agent
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .map(|value| BrowserProfile::Custom(value.trim().to_string()))
            .unwrap_or_else(|| {
                self.user_agent_profile
                    .map(BrowserProfile::from)
                    .unwrap_or_default()
            })
    }

    pub(super) fn proxy_countries_hint(&self) -> Vec<String> {
        normalize_proxy_countries(self.proxy_countries.clone())
    }

    /// Resolves collection parameters in template-capable HTTP options.
    pub fn resolve_collection_params(
        &mut self,
        context: &str,
        query_name: &str,
        params: &HashMap<String, String>,
    ) -> Result<()> {
        if let Some(user_agent) = self.user_agent.as_mut() {
            *user_agent = query_helpers::resolve_required_template(
                context,
                query_name,
                "http.user_agent",
                user_agent,
                params,
            )?;
        }
        self.proxy_countries = resolve_proxy_countries_templates(&self.proxy_countries, params);
        Ok(())
    }
}

pub(super) fn normalize_country_code(country: &str) -> String {
    country.trim().to_ascii_uppercase()
}

pub(super) fn normalize_proxy_countries(
    countries: impl IntoIterator<Item = String>,
) -> Vec<String> {
    let mut countries: Vec<_> = countries
        .into_iter()
        .map(|country| normalize_country_code(&country))
        .filter(|country| {
            country.len() == 2 && country.bytes().all(|byte| byte.is_ascii_alphabetic())
        })
        .collect();
    countries.sort_unstable();
    countries.dedup();
    countries
}

/// Resolves each country-list template and expands JSON array parameters.
///
/// A runtime parameter such as `proxy_countries` is supplied as a JSON array,
/// so YAML queries can declare `proxy_countries: ["{proxy_countries}"]` while
/// static country entries remain supported in the same list.
pub(super) fn resolve_proxy_countries_templates(
    countries: &[String],
    params: &HashMap<String, String>,
) -> Vec<String> {
    let mut resolved = countries.iter().fold(Vec::new(), |mut resolved, country| {
        let (value, missing_keys) = query_helpers::replace_template_placeholders(country, params);

        if !missing_keys.is_empty() {
            if !value.trim().is_empty() && !resolved.contains(&value) {
                resolved.push(value);
            }
            return resolved;
        }

        let values = serde_json::from_str::<Vec<String>>(&value).unwrap_or_else(|_| vec![value]);
        for country in normalize_proxy_countries(values) {
            if !resolved.contains(&country) {
                resolved.push(country);
            }
        }
        resolved
    });
    resolved.sort_unstable();
    resolved
}

#[cfg(test)]
#[path = "../../../tests/scrapyfy/http_client/configuration.rs"]
mod tests;
