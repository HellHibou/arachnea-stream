use regex::Regex;
use std::sync::OnceLock;

/// Applies the `html_to_text` scraper action, converting HTML to plain text
/// using `quick_html2md` with every markdown-producing feature disabled.
///
/// # Arguments
///
/// * `selected` - HTML node targeted by the entry. When set, the action
///   converts the node's inner HTML. When `None`, every existing string
///   value in `texts` is converted independently.
/// * `texts` - Values produced by previous actions in the pipeline (used as
///   the conversion source when `selected` is `None`).
///
/// # Returns
///
/// A new value list with the HTML-converted plain text. Empty outputs are
/// dropped.
pub(super) fn apply(
    selected: &Option<scraper::ElementRef<'_>>,
    texts: Vec<String>,
) -> Vec<String> {
    let options = text_only_markdown_options();

    if let Some(el) = selected {
        // HTML scraper path: convert the selected element's HTML to plain text.
        let html = preprocess_html_breaks(&el.html());
        let text = html_to_plain_text(&html, &options);
        return if text.is_empty() {
            Vec::new()
        } else {
            vec![text]
        };
    }

    // JSON scraper path (or any other source that already produced string values):
    // convert every existing value that contains HTML into plain text.
    texts
        .into_iter()
        .map(|value| {
            let html = preprocess_html_breaks(&value);
            html_to_plain_text(&html, &options)
        })
        .filter(|value| !value.is_empty())
        .collect()
}

/// Builds a [`quick_html2md::MarkdownOptions`] instance with every
/// markdown-producing feature disabled so the converter behaves like a plain
/// text extractor.
fn text_only_markdown_options() -> quick_html2md::MarkdownOptions {
    quick_html2md::MarkdownOptions::new()
        .preserve_headings(false)
        .include_links(false)
        .include_images(false)
        .preserve_emphasis(false)
        .preserve_strikethrough(false)
        .preserve_lists(true)
        .preserve_code(false)
        .preserve_blockquotes(false)
        .preserve_tables(true)
}

/// Replaces `<br>`, `<br/>`, and `<br />` tags with newline markers before
/// conversion so the resulting text keeps the original line breaks.
///
/// # Arguments
///
/// * `html` - Raw HTML fragment to normalize.
///
/// # Returns
///
/// The HTML fragment with every `<br>` tag replaced by `\n`.
fn preprocess_html_breaks(html: &str) -> String {
    static BR_REGEX: OnceLock<Regex> = OnceLock::new();
    let regex = BR_REGEX.get_or_init(|| {
        Regex::new(r"(?i)<br\s*/?>").expect("Invalid <br> regex")
    });
    regex.replace_all(html, "\n").into_owned()
}

/// Converts an HTML fragment to plain text using `quick_html2md` with all
/// markdown features disabled, then normalizes whitespace and trims the result.
///
/// # Arguments
///
/// * `html` - HTML fragment to convert (already pre-processed for `<br>`).
/// * `options` - Markdown options returned by
///   [`text_only_markdown_options`].
///
/// # Returns
///
/// The plain-text representation of `html`, with empty lines collapsed and
/// each remaining line trimmed.
fn html_to_plain_text(
    html: &str,
    options: &quick_html2md::MarkdownOptions,
) -> String {
    if html.trim().is_empty() {
        return String::new();
    }

    let markdown = quick_html2md::html_to_markdown_with_options(html, options);

    let mut result = String::new();
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(trimmed);
    }

    result
}
