//! Polymorphic HTML, JSON, text and static row extraction.

use super::*;

/// Downcasts a `&dyn ScraperEntrySpec` to `&HtmlScraperEntry` via
/// [`std::any::Any`].
///
/// Used by the unified executor to access HTML-specific entry functionality
/// when processing HTML queries.
///
/// # Arguments
///
/// * `entry` - The entry to downcast.
///
/// # Returns
///
/// `Some(&HtmlScraperEntry)` if the entry is an HTML entry, `None` otherwise.
pub(super) fn as_html_entry(entry: &dyn ScraperEntrySpec) -> Option<&HtmlScraperEntry> {
    entry.as_any().downcast_ref::<HtmlScraperEntry>()
}

/// Downcasts a `&dyn ScraperEntrySpec` to `&JsonScraperEntry` via
/// [`std::any::Any`].
///
/// Used by the unified executor to access JSON-specific entry functionality
/// when processing JSON queries.
///
/// # Arguments
///
/// * `entry` - The entry to downcast.
///
/// # Returns
///
/// `Some(&JsonScraperEntry)` if the entry is a JSON entry, `None` otherwise.
pub(super) fn as_json_entry(entry: &dyn ScraperEntrySpec) -> Option<&JsonScraperEntry> {
    entry.as_any().downcast_ref::<JsonScraperEntry>()
}

/// Extracts query-level context fields from the current JSON context row.
///
/// # Arguments
///
/// * `query` - Query definition supplying request, extraction and post-processing settings.
/// * `row` - Current source row from which fields or context values are extracted.
/// * `context` - Execution context supplying runtime parameters, parent response and request metadata.
/// * `request_url` - URL of the current response, used as the base for URL and header templates.
pub(super) fn extract_json_context_entries(
    query: &dyn ScraperQuery,
    row: &Value,
    context: &QueryContext<'_>,
    request_url: &str,
) -> Result<ScraperDataNode> {
    let mut item = ScraperDataNode::default();
    for entry in query.context_entries() {
        if let Some(json_entry) = as_json_entry(entry) {
            json_entry.apply_to(
                &mut item,
                row,
                context.params,
                context.dynamic_template_variables,
                request_url,
            )?;
        }
    }
    Ok(item)
}

/// Returns JSON row filters for a concrete JSON query or sub-query.
///
/// # Arguments
///
/// * `query` - Query definition supplying request, extraction and post-processing settings.
pub(super) fn json_row_filters(query: &dyn ScraperQuery) -> Option<&HashMap<String, Vec<String>>> {
    if let Some(json_query) = query.as_any().downcast_ref::<JsonScraperQuery>() {
        return Some(&json_query.row_filters);
    }

    query
        .as_any()
        .downcast_ref::<JsonScraperSubQuery>()
        .map(|json_query| &json_query.row_filters)
}

/// Resolves runtime placeholders in an HTML row selector.
///
/// Row selectors are templates so a query can scope its rows with runtime
/// parameters, typically `{url_fragment}` when the caller's link carries a
/// fragment (`page.html#ep3`). Unknown placeholders are kept verbatim, which
/// leaves the selector matching nothing instead of failing the whole query.
///
/// # Arguments
///
/// * `selector` - Selector template coming from the query configuration.
/// * `params` - Runtime template parameters.
pub(super) fn resolve_html_selector(selector: &str, params: &HashMap<String, String>) -> String {
    query_helpers::replace_template_placeholders(selector, params).0
}

/// Extracts items by applying entries to each row from the fetched response.
///
/// Returns owned [`ScraperDataNode`] items (HTML rows own their data; JSON
/// rows borrow the underlying `Value`). The result is consumed by the
/// sibling sub-query loop in [`execute_query_internal`].
///
/// # Arguments
///
/// * `query` - The query being executed.
/// * `response` - The fetched response to extract items from.
/// * `request_url` - The URL of the request that produced this response.
/// * `context` - Runtime context containing parameters.
///
/// # Returns
///
/// A vector of [`ScraperDataNode`] items, one for each row extracted from the response.
///
/// # Errors
///
/// Returns an error when a static entry fails to apply its template
/// pipeline (e.g. an unresolvable placeholder). Other branches are
/// infallible.
pub(super) fn extract_items(
    query: &dyn ScraperQuery,
    response: &FetchedResponse,
    request_url: &str,
    context: &QueryContext<'_>,
) -> Result<Vec<ScraperDataNode>> {
    let html_body: Option<&str> = match response {
        FetchedResponse::Html(html) => Some(html.as_str()),
        _ => None,
    };

    let result = match response {
        FetchedResponse::Html(html) => {
            // HTML: parse the document and apply entries directly.
            let doc = ::scraper::Html::parse_document(html);
            let select_first = matches!(
                query.row_locator(),
                RowLocator::Selector { select, .. }
                    if matches!(select, HtmlScraperSelectMode::First)
            );
            let compiled = match &query.row_locator() {
                RowLocator::Selector { selector, .. } => selector.clone(),
                _ => return Ok(Vec::new()),
            };
            let compiled = resolve_html_selector(&compiled, context.params);
            let selector = match ::scraper::Selector::parse(&compiled) {
                Ok(sel) => sel,
                Err(_) => return Ok(Vec::new()),
            };
            let mut items = Vec::new();
            for element in doc.select(&selector) {
                let mut item = ScraperDataNode::default();
                for entry in query.entries() {
                    if let Some(html_entry) = as_html_entry(entry) {
                        html_entry.apply_to(
                            &mut item,
                            element,
                            context.params,
                            context.dynamic_template_variables,
                            request_url,
                            html_body,
                        )?;
                    }
                }
                items.push(item);
                if select_first {
                    break;
                }
            }
            items
        }
        FetchedResponse::Json(value) => {
            // JSON: walk the value via the configured pointer, then apply
            // entries to each extracted row.
            let pointer = match &query.row_locator() {
                RowLocator::Pointer(pointer) => pointer.clone(),
                _ => return Ok(Vec::new()),
            };
            let values: Vec<&Value> =
                select_json_values(value, Some(&pointer), HtmlScraperSelectMode::All);
            let mut items = Vec::new();
            for row_value in values {
                let mut item = ScraperDataNode::default();
                for entry in query.entries() {
                    if let Some(json_entry) = as_json_entry(entry) {
                        json_entry.apply_to(
                            &mut item,
                            row_value,
                            context.params,
                            context.dynamic_template_variables,
                            request_url,
                        )?;
                    }
                }
                items.push(item);
            }
            items
        }
        FetchedResponse::Static => {
            // Static: a single row carrying the entries directly.
            // Static entries are YAML-declared (no JSON row, no HTML row);
            // they are rendered through their template pipeline against
            // `params` via `StaticScraperEntryRaw::apply_to`. Falling back
            // to the JSON downcast would silently skip every entry, which
            // is the regression observed on `m6play-fr` (its
            // `service_stream_metadata` is a static query and returned an
            // empty JSON before the fix).
            let mut item = ScraperDataNode::default();
            for entry in query.entries() {
                if let Some(static_entry) = entry.as_any().downcast_ref::<StaticScraperEntryRaw>() {
                    static_entry.apply_to(
                        &mut item,
                        context.params,
                        context.dynamic_template_variables,
                    )?;
                }
            }
            vec![item]
        }
        FetchedResponse::NotModified => {
            // Nothing changed remotely — no rows to extract.
            Vec::new()
        }
        FetchedResponse::Text(text) => {
            // Text: split by row delimiter, then by field delimiter, and
            // apply text entries to each parsed row.
            let row_delimiter = query.row_delimiter().unwrap_or("\n");
            let field_delimiter = query.field_delimiter();
            let lines: Vec<&str> = text
                .split(row_delimiter)
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect();

            let mut items = Vec::new();
            for line in &lines {
                let fields: Vec<&str> = if let Some(delim) = field_delimiter {
                    line.split(delim).collect()
                } else {
                    vec![*line]
                };

                let mut item = ScraperDataNode::default();
                for entry in query.entries() {
                    if let Some(text_entry) = entry
                        .as_any()
                        .downcast_ref::<crate::scrapyfy::scraper_text::entry::TextScraperEntry>(
                    ) {
                        text_entry.apply_to(
                            &mut item,
                            &fields,
                            context.params,
                            context.dynamic_template_variables,
                            request_url,
                        )?;
                    }
                }
                items.push(item);
            }
            items
        }
    };
    Ok(result)
}

/// Extracts items and returns them alongside their raw JSON row value.
///
/// The raw row value is used as parent_response for sibling sub-queries
/// that need context_pointer resolution scoped to the row (e.g.
/// context_pointer: /widgets/* must be resolved against the row at
/// row_pointer: /data, not the full response).
///
/// For non-JSON responses (HTML, Static), the row value is None and we
/// fall back to the full response as parent.
///
/// # Arguments
///
/// * `query` - The query being executed.
/// * `response` - The fetched response to extract items from.
/// * `request_url` - The URL of the request that produced this response.
/// * `context` - Runtime context containing parameters.
///
/// # Returns
///
/// A vector of tuples (item, optional raw JSON row value), one for each
/// row extracted from the response.
///
/// # Errors
///
/// See [`extract_items`] for error conditions.
pub(super) fn extract_items_with_rows<'a>(
    query: &dyn ScraperQuery,
    response: &'a FetchedResponse,
    request_url: &str,
    context: &QueryContext<'_>,
) -> Result<Vec<(ScraperDataNode, Option<&'a Value>)>> {
    match response {
        FetchedResponse::Json(value) => {
            let pointer = match &query.row_locator() {
                RowLocator::Pointer(pointer) => pointer.clone(),
                _ => {
                    // Fall back to the original extract_items and map to (item, None).
                    return Ok(extract_items(query, response, request_url, context)?
                        .into_iter()
                        .map(|item| (item, None))
                        .collect());
                }
            };
            let values: Vec<&Value> =
                select_json_values(value, Some(&pointer), HtmlScraperSelectMode::All);
            let mut result = Vec::new();
            for row_value in values {
                let mut item = ScraperDataNode::default();
                for entry in query.entries() {
                    if let Some(json_entry) = as_json_entry(entry) {
                        json_entry.apply_to(
                            &mut item,
                            row_value,
                            context.params,
                            context.dynamic_template_variables,
                            request_url,
                        )?;
                    }
                }
                result.push((item, Some(row_value)));
            }
            Ok(result)
        }
        // Non-JSON: fall back to the original extract_items and map to (item, None).
        _ => Ok(extract_items(query, response, request_url, context)?
            .into_iter()
            .map(|item| (item, None))
            .collect()),
    }
}

/// Returns the response body as an `&str` for the given fetched response.
///
/// # Arguments
///
/// * `response` - The fetched response to extract the body from.
///
/// # Returns
///
/// `Some(&str)` if the response is HTML, `None` otherwise.
pub(super) fn response_body_str(response: &FetchedResponse) -> Option<&str> {
    match response {
        FetchedResponse::Html(html) => Some(html.as_str()),
        _ => None,
    }
}
