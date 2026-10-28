/// Applies the `get_html` scraper action by appending the selected element HTML
/// to the current value list.
///
/// # Arguments
///
/// * `selected` - Optional selected HTML element available to the action.
/// * `texts` - Values produced by earlier actions in the pipeline.
pub(super) fn apply(
    selected: &Option<scraper::ElementRef<'_>>,
    mut texts: Vec<String>,
) -> Vec<String> {
    if let Some(value) = selected.as_ref().map(|element| element.html()) {
        texts.push(value);
    }

    texts
}
