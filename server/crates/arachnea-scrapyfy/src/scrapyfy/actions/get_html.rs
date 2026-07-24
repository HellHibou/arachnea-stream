/// Applies the `get_html` scraper action by appending the selected element HTML
/// to the current value list.
pub(super) fn apply(
    selected: &Option<scraper::ElementRef<'_>>,
    mut texts: Vec<String>,
) -> Vec<String> {
    if let Some(value) = selected.as_ref().map(|element| element.html()) {
        texts.push(value);
    }

    texts
}
