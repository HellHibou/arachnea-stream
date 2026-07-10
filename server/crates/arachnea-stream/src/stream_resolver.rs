use anyhow::{bail, Context, Result};
use const_format::concatcp;
use serde::Serialize;
use std::collections::HashMap;

use arachnea_scrapyfy::*;

use crate::services::player_resolver::{ResolvedPlayerStream, SpriteThumbnail};

/// Group name used by the stream resolver configuration.
pub const STREAM_RESOLVER_GROUP_NAME: &str = "arachnea-stream-resolver";

/// Default path used by the stream resolver services configuration.
pub const STREAM_RESOLVER_CONFIG_PATH: &str = concatcp!(
    DEFAULT_SERVICES_DIRECTORY,
    "/",
    STREAM_RESOLVER_GROUP_NAME,
    "/services.json"
);

/// Name of the `can_resolve_url` YAML query.
const CAN_RESOLVE_QUERY_NAME: &str = "can_resolve_url";

/// Name of the `resolve_stream` YAML query.
const RESOLVE_STREAM_QUERY_NAME: &str = "resolve_stream";

/// Resolver identifier used by the flat YAML player descriptor.
pub const GENERIC_STREAM_RESOLVER_ID: &str = "stream-resolver";

/// Result of the `can_resolve_url` aggregation across all YAML services.
#[derive(Default)]
struct CanResolveResult {
    /// Whether at least one service matched the URL.
    can_resolve: bool,
    /// Name of the first matching service (in `services.json` order).
    resolver: Option<String>,
    /// List of all matching service names when multiple services matched.
    matched_services: Vec<String>,
}

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
}

impl<'a> StreamResolver<'a> {
    /// Creates a new resolver backed by the provided aggregator.
    ///
    /// # Arguments
    ///
    /// * `scraper_agregator` - Aggregator that has loaded the `arachnea-stream-resolver` group.
    pub fn new(scraper_agregator: &'a ScraperAgregator) -> Self {
        Self { scraper_agregator }
    }

    /// Resolves one external player URL into a playable stream or iframe fallback.
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
    /// Returns an error when a matched resolver exists but fails to extract the stream.
    /// This is intentionally distinct from the fallback: extraction failures are visible
    /// and not silently masked by the iframe fallback.
    pub async fn get_stream(&self, url: &str) -> Result<ResolvedStream> {
        // Validate the URL is HTTP(S)
        if !url.starts_with("http://") && !url.starts_with("https://") {
            bail!("Unsupported URL scheme for stream resolution: `{}`", url);
        }

        // Run can_resolve_url across all services in the group
        let can_resolve = self.aggregate_can_resolve(url).await?;

        if !can_resolve.can_resolve {
            tracing::debug!(
                url = %url,
                "No YAML resolver matched the URL, falling back to embed-link"
            );
            return Ok(ResolvedStream::EmbedLink {
                embed_link: url.to_string(),
            });
        }

        let selected_service = can_resolve.resolver.unwrap_or_default();

        if can_resolve.matched_services.len() > 1 {
            tracing::warn!(
                url = %url,
                matched = ?can_resolve.matched_services,
                selected = %selected_service,
                "Multiple YAML resolvers matched the URL; using first in services.json order"
            );
        }

        // Execute resolve_stream on the selected service
        match self.execute_resolve_stream(&selected_service, url).await {
            Ok(stream) => Ok(ResolvedStream::Stream(stream)),
            Err(error) => {
                bail!(
                    "Stream resolver `{}` failed for URL `{}`: {}",
                    selected_service,
                    url,
                    error
                );
            }
        }
    }

    /// Runs `can_resolve_url` on every service in the group and aggregates results.
    async fn aggregate_can_resolve(&self, url: &str) -> Result<CanResolveResult> {
        let mut params = HashMap::new();
        params.insert("url".to_string(), url.to_string());

        let results = self
            .scraper_agregator
            .execute_query_async(
                STREAM_RESOLVER_GROUP_NAME,
                CAN_RESOLVE_QUERY_NAME,
                &params,
                None,
                None,
                None,
                None,
                None,
            )
            .await?;

        let mut result = CanResolveResult::default();

        for entry in results {
            let resolver = entry
                .get("resolver")
                .and_then(|node| node.value_as_string())
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());

            if let Some(resolver) = resolver {
                if result.resolver.is_none() {
                    result.resolver = Some(resolver.clone());
                    result.can_resolve = true;
                }
                result.matched_services.push(resolver);
            }
        }

        Ok(result)
    }

    /// Executes `resolve_stream` on the named service with the given URL.
    async fn execute_resolve_stream(
        &self,
        service_name: &str,
        url: &str,
    ) -> Result<ResolvedPlayerStream> {
        let mut params = HashMap::new();
        params.insert("url".to_string(), url.to_string());

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

        let entry = results
            .into_iter()
            .next()
            .context("Empty stream resolution response from YAML resolver")?;

        convert_resolver_entry_to_stream(&entry, url)
    }
}

/// Converts one YAML resolver response entry into a `ResolvedPlayerStream`.
///
/// Reads `stream_url`, `stream_headers`, `manifest_type`, `vtt_url`, `storyboard`
/// and optional `license_url`/`license_headers` from the scraper data nodes.
fn convert_resolver_entry_to_stream(
    entry: &HashMap<String, ScraperDataNode>,
    source_url: &str,
) -> Result<ResolvedPlayerStream> {
    let stream_url = extract_string_list(entry, "stream_url")
        .or_else(|| extract_string_list(entry, "stream-url"))
        .filter(|list| !list.is_empty())
        .context("Missing or empty `stream_url` in resolver response")?;

    let manifest_type = extract_first_string(entry, "manifest_type")
        .or_else(|| extract_first_string(entry, "manifest-type"));

    let stream_headers = extract_string_map(entry, "stream_headers");

    let mut result = ResolvedPlayerStream {
        stream_url,
        manifest_type,
        stream_headers,
        license_url: extract_first_string(entry, "license_url"),
        license_headers: extract_string_map(entry, "license_headers"),
        vtt_url: extract_first_string(entry, "vtt_url"),
        storyboard: extract_storyboard(entry),
    };

    // Ensure stream_headers includes at least Referer = source_url when not specified
    if !result.stream_headers.contains_key("Referer") {
        result
            .stream_headers
            .insert("Referer".to_string(), source_url.to_string());
    }

    Ok(result)
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
        if values.is_empty() { None } else { Some(values) }
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
/// Reads `url`/`link`, `width`, `height`, `columns`, `interval` from child nodes.
fn extract_storyboard(entry: &HashMap<String, ScraperDataNode>) -> Option<SpriteThumbnail> {
    let storyboard = entry.get("storyboard")?;
    let url = storyboard
        .children
        .get("url")
        .and_then(|n| n.value_as_string())
        .or_else(|| storyboard.children.get("link").and_then(|n| n.value_as_string()))?;
    let url = url.trim().to_string();
    if url.is_empty() {
        return None;
    }

    let width = storyboard
        .children
        .get("width")
        .and_then(|n| n.value_as_u32())?;
    let height = storyboard
        .children
        .get("height")
        .and_then(|n| n.value_as_u32())?;
    let columns = storyboard
        .children
        .get("columns")
        .and_then(|n| n.value_as_u32())?;
    let interval = storyboard
        .children
        .get("interval")
        .and_then(|n| n.value_as_string())
        .and_then(|v| v.parse::<f64>().ok())?;

    if width == 0 || height == 0 || columns == 0 || interval <= 0.0 {
        return None;
    }

    Some(SpriteThumbnail {
        url,
        width,
        height,
        columns,
        interval,
    })
}

#[cfg(test)]
#[path = "stream_resolver_tests.rs"]
mod tests;
