use url::Url;

/// Applies the `get_url_host` scraper action by replacing every value with the
/// parsed URL host, dropping entries that are not parseable as a URL.
///
/// # Arguments
///
/// * `texts` - Values produced by previous actions in the pipeline.
///
/// # Returns
///
/// A new value list where each entry is the host of the parsed URL (with a
/// leading `www.` stripped). Entries that fail to parse as a URL or expose no
/// host are skipped.
pub(super) fn apply(texts: Vec<String>) -> Vec<String> {
    texts
        .into_iter()
        .filter_map(|value| {
            let url = Url::parse(&value).ok()?;
            let host = url.host_str()?.trim_start_matches("www.").trim();
            if host.is_empty() {
                None
            } else {
                Some(host.to_string())
            }
        })
        .collect()
}
