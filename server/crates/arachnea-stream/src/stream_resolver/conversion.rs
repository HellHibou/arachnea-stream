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
/// Returns an error when the structured stream has no usable sources.
///
/// # Returns
/// The resolved stream with metadata and optional playback extras.
pub(super) fn convert_resolver_entry_to_stream(
    entry: &HashMap<String, ScraperDataNode>,
    source_url: &str,
) -> Result<ResolvedPlayerStream> {
    let sources = entry
        .get("streams")
        .context("Missing `streams` in resolver response")?
        .to_json_value()
        .map_err(anyhow::Error::msg)?;
    let sources = sources.as_array().context("`streams` must be an array")?;
    let mut stream = crate::services::player_resolver::PlayerStream {
        streams: sources
            .iter()
            .filter_map(|value| {
                serde_json::from_value::<crate::services::player_resolver::ResolvedStreamSource>(
                    value.clone(),
                )
                .ok()
            })
            .filter(|source| {
                !source.url.trim().is_empty()
                    && matches!(
                        source.source_type.as_str(),
                        "hls" | "dash" | "mp4" | "webm" | "ogg"
                    )
            })
            .collect(),
        storyboard_vtt_url: extract_first_string(entry, "storyboard_vtt_url"),
        storyboard: extract_storyboard(entry),
        chapters: extract_chapters(entry),
        subtitles: extract_subtitles(entry),
    };
    if stream.streams.is_empty() {
        bail!("Missing usable `streams` in resolver response");
    }
    for source in &mut stream.streams {
        source
            .stream_headers
            .entry("Referer".to_string())
            .or_insert_with(|| source_url.to_string());
    }
    let result = ResolvedPlayerStream {
        title: extract_first_string(entry, "title"),
        image_title: extract_image_title(entry),
        stream,
    };

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
    let link = image_title.first_child_value("link")?.trim().to_string();

    if link.is_empty() {
        return None;
    }

    Some(ResolvedPlayerImageTitle { link })
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
    entry.get(key)?.trimmed_value().map(str::to_string)
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
    let url = storyboard
        .first_child_value("url")
        .or_else(|| storyboard.first_child_value("link"))?;
    let url = url.trim().to_string();
    if url.is_empty() {
        return None;
    }

    let width = storyboard
        .first_child_value("width")
        .and_then(|value| value.parse::<u32>().ok());
    let height = storyboard
        .first_child_value("height")
        .and_then(|value| value.parse::<u32>().ok());
    let columns = storyboard
        .first_child_value("columns")?
        .parse::<u32>()
        .ok()?;
    let rows = storyboard.first_child_value("rows")?.parse::<u32>().ok()?;
    let first_page_index = storyboard
        .first_child_value("first_page_index")
        .and_then(|value| value.parse::<u32>().ok());
    let index_padding = storyboard
        .first_child_value("index_padding")
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0);
    let interval = storyboard
        .first_child_value("interval")
        .and_then(|value| value.parse::<f64>().ok());

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
        index_padding,
        interval,
    })
}

/// Extracts ordered external subtitle tracks from a resolver entry.
///
/// Reads every `subtitles` item and keeps tracks with a usable `link`.
/// `lang` and `label` stay optional; blank values normalize to `None`.
/// `auto_translate` is an optional boolean flag parsed from its child value.
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
    let link = item.first_child_value("link")?.trim().to_string();
    if !is_browser_consumable_subtitle_link(&link) {
        return None;
    }

    Some(ResolvedPlayerSubtitle {
        lang: normalize_optional_child_value(item, "lang"),
        label: normalize_optional_child_value(item, "label"),
        link,
        auto_translate: item
            .first_child_value("auto_translate")
            .and_then(|value| value.trim().parse::<bool>().ok()),
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
    node.first_child_value(name)
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

/// Extracts an ordered list of chapters from a resolver entry.
///
/// Reads every `chapters` item and keeps entries with finite, strictly
/// increasing `start`/`end` values and a non-empty `type` (or
/// `type`). `title` stays optional; blank values normalize to `None`.
/// Order follows the resolver output.
///
/// # Arguments
/// * `entry` - Resolver response fields.
///
/// # Returns
/// The accepted chapters in resolver order, or `None` when none are valid.
pub(super) fn extract_chapters(entry: &HashMap<String, ScraperDataNode>) -> Option<Vec<Chapter>> {
    let node = entry.get("chapters")?;
    let mut chapters = Vec::new();

    if node.items.is_empty() {
        if let Some(chapter) = build_chapter(node) {
            chapters.push(chapter);
        }
    } else {
        for item in &node.items {
            if let Some(chapter) = build_chapter(item) {
                chapters.push(chapter);
            }
        }
    }

    if chapters.is_empty() {
        None
    } else {
        Some(chapters)
    }
}

/// Builds one chapter from a `chapters` item node.
///
/// Returns `None` when `start` or `end` is missing, non-numeric, non-finite,
/// or not strictly increasing, or when the chapter type is missing or blank.
///
/// # Arguments
/// * `item` - Chapter item node.
///
/// # Returns
/// The normalized chapter, or `None` when unusable.
fn build_chapter(item: &ScraperDataNode) -> Option<Chapter> {
    let start = item
        .first_child_value("start")?
        .trim()
        .parse::<f64>()
        .ok()?;
    let end = item
        .first_child_value("end")
        .or_else(|| item.first_child_value("end"))?
        .trim()
        .parse::<f64>()
        .ok()?;
    if !start.is_finite() || !end.is_finite() || end <= start {
        return None;
    }

    let chapter_type = item.first_child_value("type")?.trim().to_string();
    if chapter_type.is_empty() {
        return None;
    }

    let title = item
        .first_child_value("title")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    Some(Chapter {
        start,
        end,
        title,
        chapter_type,
    })
}
