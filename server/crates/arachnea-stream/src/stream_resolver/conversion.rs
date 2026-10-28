//! Conversion of extracted YAML rows into player stream metadata.

use super::*;

/// Converts one YAML resolver response entry into a `ResolvedPlayerStream`.
///
/// Reads stream URLs, headers, player metadata, and optional playback extras from scraper data nodes.
///
/// # Arguments
/// * `entry` - Resolver response fields keyed by their scraper field name.
/// * `source_url` - Source page URL used as the default `Referer` header.
///
/// # Errors
/// Returns an error when the entry has no non-empty `stream_url`.
///
/// # Returns
/// The resolved stream with metadata and optional playback extras.
pub(super) fn convert_resolver_entry_to_stream(
    entry: &HashMap<String, ScraperDataNode>,
    source_url: &str,
) -> Result<ResolvedPlayerStream> {
    let stream_url = extract_string_list(entry, "stream_url")
        .or_else(|| extract_string_list(entry, "stream-url"))
        .filter(|list| !list.is_empty());

    if stream_url.is_none() {
        tracing::debug!(target: "arachnea_stream::stream_resolver",
            source_url = %source_url,
            keys = ?entry.keys().collect::<Vec<_>>(),
            "Failed to extract stream_url from resolver response"
        );
        for (key, node) in entry {
            tracing::debug!(target: "arachnea_stream::stream_resolver",
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
        subtitles: extract_subtitles(entry),
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
///
/// # Arguments
/// * `entry` - Resolver response fields.
///
/// # Returns
/// The image title link, or `None` when absent or blank.
pub(super) fn extract_image_title(
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
///
/// # Arguments
/// * `node` - Scraper data node to inspect.
/// * `name` - Child field name.
///
/// # Returns
/// The first resolvable child value, or `None` when absent.
pub(super) fn first_child_value<'a>(node: &'a ScraperDataNode, name: &str) -> Option<&'a str> {
    node.children
        .get(name)
        .or_else(|| node.items.first()?.children.get(name))?
        .value_as_string()
}

/// Extracts an ordered list of strings from a scraper data entry field.
/// Uses the `values` vector which contains all scalar values for a node.
///
/// # Arguments
/// * `entry` - Resolver response fields.
/// * `key` - Field name to read.
///
/// # Returns
/// The trimmed non-empty values, or `None` when none are present.
pub(super) fn extract_string_list(
    entry: &HashMap<String, ScraperDataNode>,
    key: &str,
) -> Option<Vec<String>> {
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
///
/// # Arguments
/// * `entry` - Resolver response fields.
/// * `key` - Field name to read.
///
/// # Returns
/// The trimmed non-empty first value, or `None`.
pub(super) fn extract_first_string(
    entry: &HashMap<String, ScraperDataNode>,
    key: &str,
) -> Option<String> {
    entry
        .get(key)
        .and_then(|node| node.value_as_string())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Extracts the non-empty normalized `error_message` from a resolver response entry.
///
/// Reuses `extract_first_string` so surrounding whitespace is trimmed and blank
/// values are ignored. `None` means the entry carries no business error.
///
/// # Arguments
/// * `entry` - Resolver response fields.
///
/// # Returns
/// The normalized error message, or `None`.
pub(super) fn extract_error_message(entry: &HashMap<String, ScraperDataNode>) -> Option<String> {
    extract_first_string(entry, "error_message")
}

/// Extracts a string-to-string map from a scraper data entry field.
/// Iterates over child nodes of the named field.
///
/// # Arguments
/// * `entry` - Resolver response fields.
/// * `key` - Field name containing the map.
///
/// # Returns
/// The non-empty trimmed child values keyed by child name.
pub(super) fn extract_string_map(
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
/// Reads `url`/`link`, optional dimensions, grid metadata, and interval from child nodes.
///
/// # Arguments
/// * `entry` - Resolver response fields.
///
/// # Returns
/// The storyboard when its URL and required grid values are valid.
pub(super) fn extract_storyboard(
    entry: &HashMap<String, ScraperDataNode>,
) -> Option<SpriteThumbnail> {
    let storyboard = entry.get("storyboard")?;
    let url =
        first_child_value(storyboard, "url").or_else(|| first_child_value(storyboard, "link"))?;
    let url = url.trim().to_string();
    if url.is_empty() {
        return None;
    }

    let width = first_child_value(storyboard, "width").and_then(|value| value.parse::<u32>().ok());
    let height =
        first_child_value(storyboard, "height").and_then(|value| value.parse::<u32>().ok());
    let columns = first_child_value(storyboard, "columns")?
        .parse::<u32>()
        .ok()?;
    let rows = first_child_value(storyboard, "rows")?.parse::<u32>().ok()?;
    let first_page_index = first_child_value(storyboard, "first_page_index")
        .and_then(|value| value.parse::<u32>().ok());
    let interval =
        first_child_value(storyboard, "interval").and_then(|value| value.parse::<f64>().ok());

    if width.is_some_and(|value| value == 0)
        || height.is_some_and(|value| value == 0)
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

/// Extracts ordered external subtitle tracks from a resolver entry.
///
/// Reads every `subtitles` item and keeps tracks with a usable `link`.
/// `lang` and `label` stay optional; blank values normalize to `None`.
/// Order follows the resolver output and exact `(lang, label, link)`
/// duplicates are dropped.
///
/// # Arguments
/// * `entry` - Resolver response fields.
///
/// # Returns
/// The accepted subtitle tracks in resolver order.
pub(super) fn extract_subtitles(
    entry: &HashMap<String, ScraperDataNode>,
) -> Vec<ResolvedPlayerSubtitle> {
    let Some(node) = entry.get("subtitles") else {
        return Vec::new();
    };

    let mut subtitles = Vec::new();
    if node.items.is_empty() {
        if let Some(subtitle) = build_subtitle(node) {
            subtitles.push(subtitle);
        }
        return subtitles;
    }

    for item in &node.items {
        let Some(subtitle) = build_subtitle(item) else {
            continue;
        };
        let is_duplicate = subtitles.iter().any(|existing: &ResolvedPlayerSubtitle| {
            existing.link == subtitle.link
                && existing.lang == subtitle.lang
                && existing.label == subtitle.label
        });
        if !is_duplicate {
            subtitles.push(subtitle);
        }
    }

    subtitles
}

/// Builds one subtitle track from a `subtitles` item node.
/// Returns `None` when `link` is missing or not browser-consumable.
///
/// # Arguments
/// * `item` - Subtitle item node.
///
/// # Returns
/// The normalized subtitle track, or `None` when unusable.
pub(super) fn build_subtitle(item: &ScraperDataNode) -> Option<ResolvedPlayerSubtitle> {
    let link = first_child_value(item, "link")?.trim().to_string();
    if !is_browser_consumable_subtitle_link(&link) {
        return None;
    }

    Some(ResolvedPlayerSubtitle {
        lang: normalize_optional_child_value(item, "lang"),
        label: normalize_optional_child_value(item, "label"),
        link,
    })
}

/// Returns the trimmed child value, normalizing blank values to `None`.
///
/// # Arguments
/// * `node` - Parent scraper data node.
/// * `name` - Child field name.
///
/// # Returns
/// The trimmed non-empty value, or `None`.
pub(super) fn normalize_optional_child_value(node: &ScraperDataNode, name: &str) -> Option<String> {
    first_child_value(node, name)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Returns whether a subtitle link is directly consumable by the browser.
///
/// Accepts absolute HTTP(S) URLs and root-relative application paths such as
/// the backend HTTP proxy. Anything else is ignored so resolver payloads can
/// never smuggle headers, tokens, or non-web schemes into the public JSON.
///
/// # Arguments
/// * `link` - Candidate subtitle URL.
///
/// # Returns
/// `true` for HTTP(S) URLs and root-relative paths.
pub(super) fn is_browser_consumable_subtitle_link(link: &str) -> bool {
    link.starts_with("http://")
        || link.starts_with("https://")
        || (link.starts_with('/') && !link.starts_with("//"))
}
