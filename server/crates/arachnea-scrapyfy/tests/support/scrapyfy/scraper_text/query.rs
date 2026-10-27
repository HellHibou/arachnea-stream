use super::*;

impl TextScraperQuery {
    /// Returns the flattened list of leaf field names produced by this query.
    pub fn get_field_names(&self) -> Vec<String> {
        self.scraper_entries
            .iter()
            .flat_map(|e| {
                let mut names = Vec::new();
                collect_text_field_names(e, None, &mut names);
                names
            })
            .collect()
    }
}

#[cfg(any(test, feature = "test-support"))]
fn collect_text_field_names(
    entry: &crate::scrapyfy::scraper_text::entry::TextScraperEntry,
    prefix: Option<&str>,
    names: &mut Vec<String>,
) {
    let full_name = match prefix {
        Some(prefix) => format!("{} > {}", prefix, entry.name()),
        None => entry.name().to_string(),
    };
    match entry {
        crate::scrapyfy::scraper_text::entry::TextScraperEntry::Field { .. } => {
            names.push(full_name)
        }
        crate::scrapyfy::scraper_text::entry::TextScraperEntry::Group { entries, .. } => {
            for child in entries {
                collect_text_field_names(child, Some(&full_name), names);
            }
        }
    }
}
