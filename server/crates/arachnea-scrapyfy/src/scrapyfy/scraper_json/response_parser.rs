//! JSON response parsing into rows.
//!
//! This module provides helpers to collect and order JSON rows from a
//! fetched response, and to filter rows against a set of conditions.

use std::collections::HashMap;

use anyhow::Result;

use crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode;
use crate::scrapyfy::scraper_json::entry::{json_value_to_strings, select_json_values};

/// Restores the original iteration order after collecting indexed async results.
///
/// This is used by sub-query execution to preserve the configuration order
/// of contexts/fetches/siblings after concurrent processing with
/// `buffer_unordered`.
///
/// # Type Parameters
///
/// * `T` - The type of values being collected.
///
/// # Arguments
///
/// * `results` - Vector of `(index, value)` pairs whose index reflects the
///   original configuration order.
///
/// # Errors
///
/// Returns the first error encountered while unwrapping the results.
pub(crate) fn collect_ordered_results<T>(results: Vec<Result<(usize, T)>>) -> Result<Vec<T>> {
    let mut ordered = Vec::with_capacity(results.len());

    for result in results {
        ordered.push(result?);
    }

    ordered.sort_by_key(|(index, _)| *index);

    Ok(ordered.into_iter().map(|(_, value)| value).collect())
}

/// Checks whether a JSON row satisfies every filter condition.
///
/// Tests a JSON value against a set of filter conditions. A row matches if it
/// satisfies all filter conditions (logical AND between all filter entries).
///
/// # Arguments
///
/// * `row` - JSON value to test against the filter map.
/// * `filters` - Map of JSON pointer to expected value strings; every entry must match.
///
/// # Returns
///
/// `true` if the row matches all filter conditions, `false` otherwise.
pub(crate) fn matches(row: &serde_json::Value, filters: &HashMap<String, Vec<String>>) -> bool {
    filters.iter().all(|(pointer, expected_values)| {
        let actual_values =
            select_json_values(row, Some(pointer.as_str()), HtmlScraperSelectMode::All)
                .into_iter()
                .flat_map(json_value_to_strings)
                .collect::<Vec<_>>();

        expected_values
            .iter()
            .any(|expected| actual_values.iter().any(|actual| actual == expected))
    })
}