/// Applies the `get_response_body` scraper action by appending the raw HTTP
/// response body to the current value list when one is available.
///
/// # Arguments
///
/// * `texts` - Values produced by previous actions in the pipeline.
/// * `response_body` - Raw response body of the current request, or `None`
///   when unavailable.
///
/// # Returns
///
/// A new value list with `response_body` appended, or the original `texts`
/// list when no body is available.
pub(super) fn apply(mut texts: Vec<String>, response_body: Option<&str>) -> Vec<String> {
    if let Some(response_body) = response_body {
        texts.push(response_body.to_string());
    }

    texts
}
