/// Applies the `get_text` scraper action by appending the trimmed inner text
/// of `selected` to the current value list.
///
/// # Arguments
///
/// * `selected` - HTML node targeted by the entry. When `None`, the action
///   leaves the value list unchanged.
/// * `texts` - Values produced by previous actions in the pipeline.
///
/// # Returns
///
/// A new value list with the trimmed inner text of `selected` appended, or
/// the original `texts` list when no element was selected.
pub(super) fn apply(
    selected: &Option<scraper::ElementRef<'_>>,
    mut texts: Vec<String>,
) -> Vec<String> {
    if let Some(value) = selected
        .as_ref()
        .map(|el| el.text().collect::<String>().trim().to_string())
    {
        texts.push(value);
    }

    texts
}
