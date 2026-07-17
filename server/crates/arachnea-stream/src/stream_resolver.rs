use anyhow::{bail, Context, Result};
use const_format::concatcp;
use serde::Serialize;
use std::collections::HashMap;

use arachnea_proxy::http::proxy_service::proxied_url;
use arachnea_scrapyfy::*;

use crate::services::player_resolver::{
    PlayerResolverEndpoints, ResolvedPlayerImageTitle, ResolvedPlayerStream, SpriteThumbnail,
};

const MAX_EMBED_HTML_BYTES: usize = 1_048_576;
const STREAM_RESOLVER_MAX_REDIRECTS: usize = 16;

/// Group name used by the stream resolver configuration.
pub const STREAM_RESOLVER_GROUP_NAME: &str = "arachnea-stream-resolver";

/// Default path used by the stream resolver services configuration.
pub const STREAM_RESOLVER_CONFIG_PATH: &str = concatcp!(
    DEFAULT_SERVICES_DIRECTORY,
    "/",
    STREAM_RESOLVER_GROUP_NAME,
    "/services.json"
);

/// Name of the `resolve_stream` YAML query.
const RESOLVE_STREAM_QUERY_NAME: &str = "resolve_stream";

/// Name of the optional `can_resolve_url` YAML query.
const CAN_RESOLVE_URL_QUERY_NAME: &str = "can_resolve_url";

/// Name of the optional `can_resolve_html` YAML query.
const CAN_RESOLVE_HTML_QUERY_NAME: &str = "can_resolve_html";

/// Resolver identifier used by the flat YAML player descriptor.
pub const GENERIC_STREAM_RESOLVER_ID: &str = "stream-resolver";

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
    },
}

/// Generic YAML-driven stream resolver facade.
///
/// Uses the `arachnea-stream-resolver` group loaded into a `ScraperAgregator` to match
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
    /// * `scraper_agregator` - Aggregator that has loaded the `arachnea-stream-resolver` group.
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
    /// produces a valid stream. When no service matches, returns an `EmbedLink`
    /// fallback.
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
    /// Returns an error when all services have been tried and none produced a
    /// valid stream, but the fallback has already been applied as `EmbedLink`.
    /// Errors from individual services (timeout, DNS, HTTP failure) are logged
    /// and do not abort the search.
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
                Ok(Some(stream)) => {
                    tracing::debug!(
                        service = %name,
                        url = %url,
                        "Stream resolved by YAML service"
                    );
                    return Ok(ResolvedStream::Stream(stream));
                }
                Ok(None) => {
                    tracing::debug!(
                        service = %name,
                        url = %url,
                        "Service did not resolve the stream"
                    );
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
        })
    }

    /// Attempts to resolve a stream for the given service and URL.
    ///
    /// Returns `Ok(Some(stream))` on success, `Ok(None)` when the service does
    /// not have a `resolve_stream` query or its response contains no stream
    /// URL, and `Err` when the query execution itself fails.
    async fn try_resolve_stream(
        &self,
        service_name: &str,
        url: &str,
        html: Option<&str>,
    ) -> Result<Option<ResolvedPlayerStream>> {
        let params = self.build_resolver_params(url, html);

        let results = self
            .scraper_agregator
            .execute_query_async(
                STREAM_RESOLVER_GROUP_NAME,
                RESOLVE_STREAM_QUERY_NAME,
                &params,
                None,
                Some(&vec![service_name.to_string()]),
                None,
                None,
                None,
            )
            .await?;

        let entry = match results.into_iter().next() {
            Some(entry) => entry,
            None => return Ok(None),
        };

        let mut stream = match convert_resolver_entry_to_stream(&entry, url) {
            Ok(stream) => stream,
            Err(_) => return Ok(None),
        };

        // Proxy stream URLs through the HTTP proxy with embedded headers.
        if let Some(proxy_path) = self.endpoints.http_proxy_public_path.as_deref() {
            let headers: Vec<(&str, &str)> = stream
                .stream_headers
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            stream.stream_url = stream
                .stream_url
                .into_iter()
                .map(|u| proxied_url(&u, Some(proxy_path), None, &[], &headers))
                .collect();
        }

        Ok(Some(stream))
    }

    /// Runs the HTML-content fallback phase after every direct resolver attempt fails.
    async fn resolve_from_fetched_html(
        &self,
        url: &str,
        source_names: &[String],
    ) -> Result<Option<ResolvedPlayerStream>> {
        let html = self.fetch_embed_html(url).await?;

        for name in source_names {
            match self.can_resolve_html(name, url, &html).await {
                Ok(true) => {
                    tracing::debug!(
                        service = %name,
                        url = %url,
                        "HTML content recognized by YAML resolver"
                    );

                    return match self.try_resolve_stream(name, url, Some(&html)).await {
                        Ok(Some(stream)) => Ok(Some(stream)),
                        Ok(None) => bail!(
                            "HTML resolver `{}` recognized URL `{}` but produced no stream",
                            name,
                            url
                        ),
                        Err(error) => Err(error).with_context(|| {
                            format!(
                                "HTML resolver `{}` recognized URL `{}` but failed to extract stream",
                                name, url
                            )
                        }),
                    };
                }
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(
                        service = %name,
                        url = %url,
                        error = ?error,
                        "Error while checking HTML resolver compatibility, trying next service"
                    );
                }
            }
        }

        Ok(None)
    }

    /// Fetches the original embed page once for HTML-content resolver detection.
    async fn fetch_embed_html(&self, url: &str) -> Result<String> {
        let http_config = ScraperHttpConfig {
            max_redirects: Some(STREAM_RESOLVER_MAX_REDIRECTS),
            ..ScraperHttpConfig::default()
        };
        let client = self
            .scraper_agregator
            .create_http_client(http_config);
        let response = client
            .send_for_request(http::Method::GET, url, &HashMap::new(), None)
            .await
            .with_context(|| format!("Failed to fetch embed HTML `{}`", url))?;

        if !response.status().is_success() {
            bail!(
                "Failed to fetch embed HTML `{}`: HTTP status {}",
                url,
                response.status()
            );
        }

        let content_type = response
            .headers()
            .get(http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        let mime_type = content_type
            .split(';')
            .next()
            .map(str::trim)
            .unwrap_or_default();

        if !mime_type.eq_ignore_ascii_case("text/html") {
            bail!(
                "Failed to inspect embed HTML `{}`: unsupported Content-Type `{}`",
                url,
                content_type
            );
        }

        let bytes = response.bytes().await?;
        if bytes.len() > MAX_EMBED_HTML_BYTES {
            bail!(
                "Failed to inspect embed HTML `{}`: body is {} bytes, limit is {} bytes",
                url,
                bytes.len(),
                MAX_EMBED_HTML_BYTES
            );
        }

        response.text().await.map_err(Into::into)
    }

    /// Runs optional `can_resolve_html` on one service with `{url, html}` params.
    async fn can_resolve_html(&self, service_name: &str, url: &str, html: &str) -> Result<bool> {
        let params = self.build_resolver_params(url, Some(html));
        let results = self
            .scraper_agregator
            .execute_query_async(
                STREAM_RESOLVER_GROUP_NAME,
                CAN_RESOLVE_HTML_QUERY_NAME,
                &params,
                None,
                Some(&vec![service_name.to_string()]),
                None,
                None,
                None,
            )
            .await?;

        Ok(results.into_iter().any(|entry| {
            entry
                .get("resolver")
                .and_then(|node| node.value_as_string())
                .is_some_and(|value| !value.trim().is_empty())
        }))
    }

    /// Runs optional `can_resolve_url` on one service as a cheap direct-resolution prefilter.
    async fn can_resolve_url(&self, service_name: &str, url: &str) -> Result<bool> {
        let params = self.build_resolver_params(url, None);
        let results = self
            .scraper_agregator
            .execute_query_async(
                STREAM_RESOLVER_GROUP_NAME,
                CAN_RESOLVE_URL_QUERY_NAME,
                &params,
                None,
                Some(&vec![service_name.to_string()]),
                None,
                None,
                None,
            )
            .await?;

        Ok(results.into_iter().any(|entry| {
            entry
                .get("resolver")
                .and_then(|node| node.value_as_string())
                .is_some_and(|value| !value.trim().is_empty())
        }))
    }

    /// Builds common runtime parameters for resolver queries.
    fn build_resolver_params(&self, url: &str, html: Option<&str>) -> HashMap<String, String> {
        let mut params = HashMap::new();
        params.insert("url".to_string(), url.to_string());
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

/// Converts one YAML resolver response entry into a `ResolvedPlayerStream`.
///
/// Reads stream URLs, headers, player metadata, and optional playback extras from scraper data nodes.
fn convert_resolver_entry_to_stream(
    entry: &HashMap<String, ScraperDataNode>,
    source_url: &str,
) -> Result<ResolvedPlayerStream> {
    let stream_url = extract_string_list(entry, "stream_url")
        .or_else(|| extract_string_list(entry, "stream-url"))
        .filter(|list| !list.is_empty());

    if stream_url.is_none() {
        tracing::debug!(
            source_url = %source_url,
            keys = ?entry.keys().collect::<Vec<_>>(),
            "Failed to extract stream_url from resolver response"
        );
        for (key, node) in entry {
            tracing::debug!(
                key = %key,
                value_count = node.values.len(),
                first_value = ?node.values.first(),
                children_keys = ?node.children.keys().collect::<Vec<_>>(),
                "Available resolver entry field"
            );
        }
    }

    let stream_url = stream_url.context("Missing or empty `stream_url` in resolver response")?;

    let manifest_type = extract_first_string(entry, "manifest_type")
        .or_else(|| extract_first_string(entry, "manifest-type"));

    let stream_headers = extract_string_map(entry, "stream_headers");

    let mut result = ResolvedPlayerStream {
        title: extract_first_string(entry, "title"),
        image_title: extract_image_title(entry),
        stream_url,
        manifest_type,
        stream_headers,
        license_url: extract_first_string(entry, "license_url"),
        license_headers: extract_string_map(entry, "license_headers"),
        storyboard_vtt_url: extract_first_string(entry, "storyboard_vtt_url"),
        storyboard: extract_storyboard(entry),
        chapters: None,
    };

    // Ensure stream_headers includes at least Referer = source_url when not specified
    if !result.stream_headers.contains_key("Referer") {
        result
            .stream_headers
            .insert("Referer".to_string(), source_url.to_string());
    }

    Ok(result)
}

/// Extracts optional `image/title > link` metadata from a resolver entry.
fn extract_image_title(
    entry: &HashMap<String, ScraperDataNode>,
) -> Option<ResolvedPlayerImageTitle> {
    let image_title = entry
        .get("image/title")
        .or_else(|| entry.get("image")?.children.get("title"))?;
    let link = first_child_value(image_title, "link")?.trim().to_string();

    if link.is_empty() {
        return None;
    }

    Some(ResolvedPlayerImageTitle { link })
}

/// Returns one named child value, including a group represented by one internal item.
fn first_child_value<'a>(node: &'a ScraperDataNode, name: &str) -> Option<&'a str> {
    node.children
        .get(name)
        .or_else(|| node.items.first()?.children.get(name))?
        .value_as_string()
}

/// Extracts an ordered list of strings from a scraper data entry field.
/// Uses the `values` vector which contains all scalar values for a node.
fn extract_string_list(entry: &HashMap<String, ScraperDataNode>, key: &str) -> Option<Vec<String>> {
    entry.get(key).and_then(|node| {
        let values: Vec<String> = node
            .values
            .iter()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect();
        if values.is_empty() {
            None
        } else {
            Some(values)
        }
    })
}

/// Extracts the first string value from a scraper data entry field.
/// Uses `value_as_string()` which returns the first value.
fn extract_first_string(entry: &HashMap<String, ScraperDataNode>, key: &str) -> Option<String> {
    entry
        .get(key)
        .and_then(|node| node.value_as_string())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Extracts a string-to-string map from a scraper data entry field.
/// Iterates over child nodes of the named field.
fn extract_string_map(
    entry: &HashMap<String, ScraperDataNode>,
    key: &str,
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(node) = entry.get(key) {
        for (child_name, child_node) in &node.children {
            if let Some(value) = child_node.value_as_string() {
                let trimmed = value.trim().to_string();
                if !trimmed.is_empty() {
                    map.insert(child_name.clone(), trimmed);
                }
            }
        }
    }
    map
}

/// Extracts an optional `SpriteThumbnail` from a scraper data entry field.
/// Reads `url`/`link`, dimensions, and interval from child nodes.
fn extract_storyboard(entry: &HashMap<String, ScraperDataNode>) -> Option<SpriteThumbnail> {
    let storyboard = entry.get("storyboard")?;
    let url =
        first_child_value(storyboard, "url").or_else(|| first_child_value(storyboard, "link"))?;
    let url = url.trim().to_string();
    if url.is_empty() {
        return None;
    }

    let width = first_child_value(storyboard, "width")?
        .parse::<u32>()
        .ok()?;
    let height = first_child_value(storyboard, "height")?
        .parse::<u32>()
        .ok()?;
    let columns = first_child_value(storyboard, "columns")?
        .parse::<u32>()
        .ok()?;
    let rows = first_child_value(storyboard, "rows")?.parse::<u32>().ok()?;
    let first_page_index = first_child_value(storyboard, "first_page_index")
        .and_then(|value| value.parse::<u32>().ok());
    let interval =
        first_child_value(storyboard, "interval").and_then(|value| value.parse::<f64>().ok());

    if width == 0
        || height == 0
        || columns == 0
        || rows == 0
        || interval.is_some_and(|value| value <= 0.0)
    {
        return None;
    }

    Some(SpriteThumbnail {
        url,
        width,
        height,
        columns,
        rows,
        first_page_index,
        interval,
    })
}

#[cfg(test)]
#[path = "stream_resolver_tests.rs"]
mod tests;
