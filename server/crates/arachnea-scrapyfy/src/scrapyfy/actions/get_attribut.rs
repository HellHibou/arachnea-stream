/// Applies the `get_attribut` scraper action by appending the value of the
/// `argument` attribute on `selected` to the current value list.
///
/// # Arguments
///
/// * `selected` - HTML node targeted by the entry. When `None`, the action
///   leaves the value list unchanged.
/// * `texts` - Values produced by previous actions in the pipeline.
/// * `argument` - Attribute name to read from the selected element (e.g.
///   `href`, `src`, `content`).
///
/// # Returns
///
/// A new value list with the attribute value appended, or the original
/// `texts` list when no element was selected or the attribute is missing.
pub(super) fn apply(
    selected: &Option<scraper::ElementRef<'_>>,
    mut texts: Vec<String>,
    argument: &str,
) -> Vec<String> {
    if let Some(value) = selected
        .as_ref()
        .and_then(|el| el.value().attr(argument).map(String::from))
    {
        texts.push(value);
    }

    texts
}
