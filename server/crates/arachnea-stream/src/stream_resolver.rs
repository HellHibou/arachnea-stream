use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::collections::HashMap;

use arachnea_core::{controler::RequestControlerContext, DEFAULT_MAX_REDIRECTS};
use arachnea_proxy::http::actions::{ProxyHttpActionConfig, ReplaceAll};
use arachnea_proxy::http::proxy_service::proxied_url_with_countries_and_insecure_tls;
use arachnea_scrapyfy::*;
use url::Url;

use crate::services::player_resolver::{
    PlayerResolverEndpoints, ResolvedPlayerImageTitle, ResolvedPlayerStream,
    ResolvedPlayerSubtitle, SpriteThumbnail,
};

use crate::STREAM_RESOLVER_MAX_EMBED_HTML_BYTES as MAX_EMBED_HTML_BYTES;

/// HTML fetched for content-based resolver recognition and its final URL after redirects.
struct FetchedEmbedHtml {
    /// Final HTTP(S) URL that delivered the document.
    url: String,
    /// Document body used by `can_resolve_html` and HTML resolvers.
    html: String,
}

/// Functional result of one `resolve_stream` attempt for a single service.
///
/// The previous binary outcome (`Some(stream)` / `None`) conflated "this service
/// produced no stream" with "this service reported an explicit business error on
/// an HTTP 200 page". Distinguishing them lets the search stop early when a
/// hoster reports a final `error_message` instead of trying other services.
enum ResolveAttemptResult {
    /// No stream was produced; the search may continue with the next service.
    NoStream,
    /// A playable stream was produced for the requested URL.
    Stream(ResolvedPlayerStream),
    /// The hoster reported a business error that must stop the search.
    /// The message is the normalized `error_message` value from the YAML entry.
    TerminalError(String),
}

pub use crate::STREAM_RESOLVER_GROUP_NAME;

pub use crate::STREAM_RESOLVER_CONFIG_PATH;

use crate::STREAM_RESOLVER_RESOLVE_STREAM_QUERY_NAME as RESOLVE_STREAM_QUERY_NAME;

use crate::STREAM_RESOLVER_CAN_RESOLVE_URL_QUERY_NAME as CAN_RESOLVE_URL_QUERY_NAME;

use crate::STREAM_RESOLVER_CAN_RESOLVE_HTML_QUERY_NAME as CAN_RESOLVE_HTML_QUERY_NAME;

pub use crate::GENERIC_STREAM_RESOLVER_ID;

pub use crate::SCRAPER_QUERY_STREAM_RESOLVER_ID;

/// Union response returned by `get_stream`.
///
/// This type mirrors the frontend `GetStreamResponse` union:
/// - `Stream(ResolvedPlayerStream)` when a YAML resolver successfully extracted the media.
/// - `EmbedLink(String)` as fallback when no YAML resolver matched the URL.
#[derive(Serialize)]
#[serde(untagged)]
pub(crate) enum ResolvedStream {
    /// Resolved media stream with URL list and metadata.
    Stream(ResolvedPlayerStream),
    /// Iframe fallback preserving the original URL for the embed player.
    EmbedLink {
        /// The iframe URL to render.
        #[serde(rename = "embed-link")]
        embed_link: String,
        /// Original player URL exposed to clients.
        #[serde(rename = "web-link")]
        web_link: String,
    },
}

/// Generic YAML-driven stream resolver facade.
///
/// Uses the `arachnea-stream-hoster` group loaded into a `ScraperAgregator` to match
/// external player URLs against configured YAML rules, then extracts playable media URLs.
/// This facade does not contain any source-specific domain logic.
pub(crate) struct StreamResolver<'a> {
    scraper_agregator: &'a ScraperAgregator,
    endpoints: &'a PlayerResolverEndpoints,
}

impl<'a> StreamResolver<'a> {
    /// Creates a new resolver backed by the provided aggregator.
    ///
    /// # Arguments
    ///
    /// * `scraper_agregator` - Aggregator that has loaded the `arachnea-stream-hoster` group.
    /// * `endpoints` - Player resolver endpoints for proxy URL generation.
    pub fn new(
        scraper_agregator: &'a ScraperAgregator,
        endpoints: &'a PlayerResolverEndpoints,
    ) -> Self {
        Self {
            scraper_agregator,
            endpoints,
        }
    }

    /// Resolves one external player URL into a playable stream or iframe fallback.
    ///
    /// Uses optional `can_resolve_url` queries as a cheap prefilter for direct
    /// `resolve_stream` attempts, then falls back to one shared HTML fetch and
    /// optional `can_resolve_html` recognition. Stops at the first service that
    /// produces a valid stream or reports a terminal `error_message`. When no
    /// service matches, returns an `EmbedLink` fallback.
    ///
    /// # Arguments
    ///
    /// * `url` - Absolute HTTP(S) URL of the external player page.
    ///
    /// # Returns
    ///
    /// * `ResolvedStream::Stream` when a YAML resolver extracted media URLs.
    /// * `ResolvedStream::EmbedLink` when no YAML resolver matched the URL.
    ///
    /// # Errors
    ///
    /// Returns an error when a selected YAML resolver returns a non-empty
    /// `error_message`. Technical failures from individual services (timeout,
    /// DNS, HTTP failure) are logged and do not abort the search.
    pub async fn get_stream(&self, url: &str) -> Result<ResolvedStream> {
        // Validate the URL is HTTP(S)
        if !url.starts_with("http://") && !url.starts_with("https://") {
            bail!("Unsupported URL scheme for stream resolution: `{}`", url);
        }

        let source_names = self
            .scraper_agregator
            .source_names_in_group(STREAM_RESOLVER_GROUP_NAME);

        for name in &source_names {
            match self.can_resolve_url(name, url).await {
                Ok(true) => {}
                Ok(false) => {
                    tracing::debug!(
                        service = %name,
                        url = %url,
                        "Service did not declare a positive can_resolve_url; skipping direct resolve_stream"
                    );
                    continue;
                }
                Err(error) => {
                    tracing::warn!(
                        service = %name,
                        url = %url,
                        error = ?error,
                        "Error while checking URL resolver compatibility, trying next service"
                    );
                    continue;
                }
            }

            match self.try_resolve_stream(name, url, None).await {
                Ok(ResolveAttemptResult::Stream(stream)) => {
                    tracing::debug!(
                        service = %name,
                        url = %url,
                        "Stream resolved by YAML service"
                    );
                    return Ok(ResolvedStream::Stream(stream));
                }
                Ok(ResolveAttemptResult::NoStream) => {
                    tracing::debug!(
                        service = %name,
                        url = %url,
                        "Service did not resolve the stream"
                    );
                }
                Ok(ResolveAttemptResult::TerminalError(message)) => {
                    tracing::debug!(
                        service = %name,
                        url = %url,
                        "Hoster reported a terminal error message, stopping the search"
                    );
                    return Err(anyhow::anyhow!(
                        "Stream hoster `{}` reported an error for URL `{}`: {}",
                        name,
                        url,
                        message
                    ));
                }
                Err(error) => {
                    tracing::warn!(
                        service = %name,
                        url = %url,
                        error = ?error,
                        "Error while resolving stream, trying next service"
                    );
                }
            }
        }

        if let Some(stream) = self.resolve_from_fetched_html(url, &source_names).await? {
            return Ok(ResolvedStream::Stream(stream));
        }

        tracing::debug!(
            url = %url,
            "No YAML resolver matched the URL or HTML content, falling back to embed-link"
        );
        Ok(ResolvedStream::EmbedLink {
            embed_link: url.to_string(),
            web_link: url.to_string(),
        })
    }

    /// Attempts to resolve a stream for the given service and URL.
    ///
    /// Returns `ResolveAttemptResult::Stream` on success,
    /// `ResolveAttemptResult::NoStream` when the service does not have a
    /// `resolve_stream` query or its response contains neither a `stream_url`
    /// nor an `error_message`, and `ResolveAttemptResult::TerminalError` when the
    /// hoster reported an explicit business error in `error_message`. Returns
    /// `Err` when the query execution itself fails (network, timeout, DNS, or
    /// Scrapyfy runtime error); these technical failures remain non-terminal.
    async fn try_resolve_stream(
        &self,
        service_name: &str,
        url: &str,
        html: Option<&str>,
    ) -> Result<ResolveAttemptResult> {
        let params = self.build_resolver_params(url, html);

        let results = self
            .scraper_agregator
            .execute_query_async(
                &RequestControlerContext::default(),
                QueryParameters {
                    cache_type: CacheType::NoCache,
                },
                STREAM_RESOLVER_GROUP_NAME,
                RESOLVE_STREAM_QUERY_NAME,
                &params,
                None,
                Some(&vec![service_name.to_string()]),
                None,
                None,
                None,
                "resolve_stream",
            )
            .await
            .data;

        let entry = match results.into_iter().next() {
            Some(entry) => entry,
            None => return Ok(ResolveAttemptResult::NoStream),
        };

        // A non-empty normalized `error_message` is terminal: it stops the
        // search before any `stream_url` conversion and is never treated as a
        // technical failure to skip.
        if let Some(error_message) = extract_error_message(&entry) {
            return Ok(ResolveAttemptResult::TerminalError(error_message));
        }

        let mut stream = match convert_resolver_entry_to_stream(&entry, url) {
            Ok(stream) => stream,
            Err(_) => return Ok(ResolveAttemptResult::NoStream),
        };

        proxy_resolved_stream(
            &mut stream,
            self.scraper_agregator,
            STREAM_RESOLVER_GROUP_NAME,
            service_name,
            self.endpoints,
            &[],
            false,
        );

        Ok(ResolveAttemptResult::Stream(stream))
    }

    /// Builds common runtime parameters for resolver queries.
    fn build_resolver_params(&self, url: &str, html: Option<&str>) -> HashMap<String, String> {
        let mut params = HashMap::new();
        params.insert("url".to_string(), url.to_string());
        if let Ok(parsed_url) = Url::parse(url) {
            params.insert(
                "origine".to_string(),
                parsed_url.origin().ascii_serialization(),
            );
        }
        if let Some(html) = html {
            params.insert("html".to_string(), html.to_string());
        }
        if let Some(proxy_path) = self
            .endpoints
            .http_proxy_public_path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            params.insert(
                HTTP_PROXY_PUBLIC_PATH_PARAM.to_string(),
                proxy_path.to_string(),
            );
        }
        params
    }
}

/// Resolves a stream through the fixed `resolve_stream` query of one stream service.
///
/// The source is selected separately from the target so callers cannot execute an
/// arbitrary query through the public stream endpoint. The target must be an absolute
/// HTTP(S) URL and is exposed to YAML as both `{url}` and `{query_url}`.
///
/// # Errors
///
/// Returns an error when the source or target is empty, the source is not loaded in
/// `arachnea-stream`, it does not declare `resolve_stream`, query execution fails, or
/// the response does not contain a usable HTTP(S) stream URL.
pub(crate) async fn resolve_scraper_query_stream(
    scraper_agregator: &ScraperAgregator,
    endpoints: &PlayerResolverEndpoints,
    source: &str,
    target: &str,
    proxy_countries: &[String],
    rewrite_manifest_urls: bool,
) -> Result<ResolvedPlayerStream> {
    let source = source.trim();
    let target = target.trim();
    if source.is_empty() {
        bail!("Missing scraper-query source.");
    }
    if target.is_empty() {
        bail!("Missing scraper-query target.");
    }

    let parsed_target = Url::parse(target).context("Invalid scraper-query target URL.")?;
    if !matches!(parsed_target.scheme(), "http" | "https") {
        bail!("Scraper-query target must use HTTP(S).");
    }

    if !scraper_agregator
        .source_names_in_group(crate::stream_scraper::STREAM_SERVICE_GROUP_NAME)
        .iter()
        .any(|name| name == source)
    {
        bail!("Unknown scraper-query source `{source}`.");
    }
    if !scraper_agregator.source_has_query(
        crate::stream_scraper::STREAM_SERVICE_GROUP_NAME,
        source,
        RESOLVE_STREAM_QUERY_NAME,
    ) {
        bail!("Scraper-query source `{source}` does not declare `{RESOLVE_STREAM_QUERY_NAME}`.");
    }

    let proxy_countries = normalize_scraper_query_proxy_countries(proxy_countries)?;
    let mut params = HashMap::from([
        ("url".to_string(), target.to_string()),
        ("query_url".to_string(), target.to_string()),
        (
            "proxy_countries".to_string(),
            serde_json::to_string(&proxy_countries).expect("normalized countries serialize"),
        ),
    ]);
    if let Some(proxy_path) = endpoints
        .http_proxy_public_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        params.insert(
            HTTP_PROXY_PUBLIC_PATH_PARAM.to_string(),
            proxy_path.to_string(),
        );
    }
    let sources = vec![source.to_string()];
    let results = scraper_agregator
        .execute_query_async(
            &RequestControlerContext::default(),
            QueryParameters::from_cache_type(CacheType::NoCache),
            crate::stream_scraper::STREAM_SERVICE_GROUP_NAME,
            RESOLVE_STREAM_QUERY_NAME,
            &params,
            None,
            Some(&sources),
            None,
            None,
            None,
            RESOLVE_STREAM_QUERY_NAME,
        )
        .await
        .data;

    let entry = results
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("Scraper-query source `{source}` returned no stream."))?;
    if let Some(message) = extract_error_message(&entry) {
        bail!("Scraper-query source `{source}` reported: {message}");
    }

    let mut stream = convert_resolver_entry_to_stream(&entry, target)
        .with_context(|| format!("Scraper-query source `{source}` returned an invalid stream."))?;
    if stream
        .stream_url
        .iter()
        .any(|url| !url.starts_with("http://") && !url.starts_with("https://"))
    {
        bail!("Scraper-query source `{source}` returned a non-HTTP(S) stream URL.");
    }

    proxy_resolved_stream(
        &mut stream,
        scraper_agregator,
        crate::stream_scraper::STREAM_SERVICE_GROUP_NAME,
        source,
        endpoints,
        &proxy_countries,
        rewrite_manifest_urls,
    );
    Ok(stream)
}

/// Validates, normalizes and deduplicates ordered country hints requested by a
/// `scraper-query` player descriptor.
fn normalize_scraper_query_proxy_countries(proxy_countries: &[String]) -> Result<Vec<String>> {
    let mut countries = Vec::new();
    for raw_country in proxy_countries {
        let country = raw_country.trim();
        if country.is_empty() {
            continue;
        }
        if country.len() != 2 || !country.bytes().all(|byte| byte.is_ascii_alphabetic()) {
            bail!(
                "Invalid scraper-query proxy country `{country}`; expected a two-letter ISO code."
            );
        }
        let country = country.to_ascii_uppercase();
        if !countries.contains(&country) {
            countries.push(country);
        }
    }
    Ok(countries)
}

/// Proxies resolved stream URLs when the HTTP proxy endpoint is configured.
///
/// # Arguments
///
/// * `proxy_countries` - Ordered ISO country hints forwarded to the proxy,
///   enabling geo-routed manifest fetches through the dynamic country pool.
/// * `rewrite_manifest_urls` - When set, a `ReplaceAll` post-response action
///   rewrites every absolute HTTP(S) URL inside proxied HLS manifests to the
///   proxy path with the current `opts` (`{proxy_inherited}`), so child
///   playlists and segments reuse the same parameters (country, Referer, …).
fn proxy_resolved_stream(
    stream: &mut ResolvedPlayerStream,
    scraper_agregator: &ScraperAgregator,
    group_name: &str,
    service_name: &str,
    endpoints: &PlayerResolverEndpoints,
    proxy_countries: &[String],
    rewrite_manifest_urls: bool,
) {
    let Some(proxy_path) = endpoints.http_proxy_public_path.as_deref() else {
        return;
    };
    let insecure_tls_hosts = scraper_agregator
        .proxy_insecure_tls_hosts(group_name, service_name)
        .unwrap_or_default();
    let actions: Vec<ProxyHttpActionConfig> = if rewrite_manifest_urls {
        vec![ReplaceAll::new(
            r"(?m)^(https?://[^\r\n]+)",
            "{proxy_inherited}/$1",
            Some(hls_manifest_content_types()),
        )]
    } else {
        Vec::new()
    };
    let headers: Vec<(&str, &str)> = stream
        .stream_headers
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    stream.stream_url = stream
        .stream_url
        .iter()
        .map(|url| {
            proxied_url_with_countries_and_insecure_tls(
                url,
                Some(proxy_path),
                proxy_countries,
                &actions,
                &headers,
                is_insecure_tls_host(url, insecure_tls_hosts),
            )
        })
        .collect();
}

/// Content types treated as HLS manifests by the proxy URL rewrite action.
fn hls_manifest_content_types() -> Vec<String> {
    [
        "application/vnd.apple.mpegurl",
        "application/x-mpegurl",
        "audio/mpegurl",
        "audio/x-mpegurl",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

/// Returns whether an absolute stream URL targets one exact allowlisted host.
fn is_insecure_tls_host(url: &str, allowed_hosts: &[String]) -> bool {
    let authority = url
        .trim()
        .strip_prefix("https://")
        .and_then(|value| value.split(['/', '?', '#']).next())
        .unwrap_or_default();
    let host = authority
        .rsplit('@')
        .next()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    allowed_hosts
        .iter()
        .any(|allowed_host| allowed_host.eq_ignore_ascii_case(&host))
}

#[cfg(test)]
#[path = "../tests/stream_resolver.rs"]
mod tests;

#[cfg(test)]
#[path = "../tests/support/stream_resolver.rs"]
mod test_support;

mod embed;

mod redirects;
use redirects::*;
mod conversion;
use conversion::*;
