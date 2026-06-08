/// Applies the `get_request_url` scraper action by appending the final
/// request URL to the current value list.
///
/// # Arguments
///
/// * `texts` - Values produced by previous actions in the pipeline.
/// * `request_url` - Final URL used to fetch the current page.
///
/// # Returns
///
/// A new value list with `request_url` appended.
pub(super) fn apply(mut texts: Vec<String>, request_url: &str) -> Vec<String> {
    texts.push(request_url.to_string());
    texts
}
