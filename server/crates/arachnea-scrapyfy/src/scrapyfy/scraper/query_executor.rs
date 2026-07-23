//! [`execute_query`] — unified polymorphic executor for every query type.
//!
//! Uses the shared types ([`ScraperQuery`], [`ScraperEntrySpec`], [`RowLocator`])
//! and trait objects for polymorphic dispatch.  Proper downcasts via
//! [`ScraperEntrySpec::as_any`], request URL/header/body resolution, post-process
//! application, field filtering, and the public [`execute_query_items`] entry point
//! used by [`ScraperQueryDefinition`](crate::scrapyfy::scraper_query_collection::ScraperQueryDefinition).

use anyhow::Result;
use futures::stream::{self, StreamExt};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

use crate::scrapyfy::post_processes::{ScraperPostProcess, ScraperPostProcessContext};
use crate::scrapyfy::query_helpers;
use crate::scrapyfy::scraper_data_node::{ScraperDataNode, ScraperOutputType};
use crate::scrapyfy::scraper_html::entry::{HtmlScraperEntry, HtmlScraperSelectMode};
use crate::scrapyfy::scraper_json::entry::{
    json_value_to_strings, select_json_values, JsonScraperEntry,
};
use crate::scrapyfy::scraper_json::query::{JsonScraperQuery, JsonScraperSubQuery};
use crate::scrapyfy::scraper_static::query::StaticScraperEntryRaw;
use crate::scrapyfy::{
    HttpClient, ScraperBrowserContext, ScraperBrowserTokenRetry, ScraperHttpExecution,
};

use super::entry_trait::ScraperEntrySpec;
use super::query_trait::ScraperQuery;
use super::row_locator::RowLocator;
use super::ScraperType;

/// Sentinel value used as `request_pointer` default for entry-level sub-queries,
/// signalling that the entry's value should be used directly as the fetch URL.
const PARENT_SENTINEL: &str = "parent";

/// Returns an empty `&[&str]` slice with explicit type annotation.
fn empty_path() -> &'static [&'static str] {
    let path: &[&str] = &[];
    path
}
/// Runtime context forwarded to polymorphic query execution.
///
/// Mirrors the runtime context used by the legacy per-type `execute_query`
/// methods; the unified executor adopts the same shape.
///
/// This context is passed through the execution chain to provide access to
/// runtime parameters, request URLs, response bodies, and HTTP clients.
pub struct QueryContext<'a> {
    /// Runtime template parameters (pre-resolved by the caller with collection
    /// defaults and `query_param_mappings` already applied).
    ///
    /// These parameters are available to actions and template resolution throughout
    /// the execution chain.
    pub params: &'a HashMap<String, String>,

    /// URL of the parent request for action resolution.
    ///
    /// Used by actions that need to reference the parent request URL.
    pub request_url: &'a str,

    /// Optional parent response body available to actions.
    ///
    /// Some actions may need access to the parent response body for processing.
    pub response_body: Option<&'a str>,

    /// Shared HTTP client.
    ///
    /// The HTTP client configured for making requests during execution.
    pub http_client: &'a HttpClient,

    /// Optional root fields filter list forwarded to `fields_filters`.
    ///
    /// When set, only fields matching the specified patterns are included in results.
    pub fields_filters: Option<&'a HashMap<String, Vec<String>>>,

    /// Optional parent response for sub-query URL resolution.
    ///
    /// When set, sub-queries with `request_pointer` can extract URLs from this response.
    pub parent_response: Option<&'a Value>,
}

/// Maximum number of follow-up requests executed together (sane default for
/// both HTML and JSON; legacy code used 8 for JSON, 4 for HTML siblings).
///
/// This controls the concurrency level for parallel HTTP requests during
/// sub-query execution.
const DEFAULT_FETCH_CONCURRENCY: usize = 8;

/// Fetches a single URL using the configured HTTP method, headers, and body.
///
/// Dispatches on `extract_next_data` for JSON queries; HTML always returns
/// the response body as text.
///
/// # Arguments
///
/// * `client` - The HTTP client to use for the request.
/// * `method` - HTTP method (GET or POST).
/// * `url` - The URL to fetch.
/// * `headers` - HTTP headers to include in the request.
/// * `body` - Optional request body.
/// * `extract_next_data` - Whether to parse the response as Next.js `__NEXT_DATA__`.
/// * `scraper_type` - The type of scraper (HTML, JSON, or Static).
///
/// # Returns
///
/// A [`FetchedResponse`] containing the parsed response data.
///
/// # Errors
///
/// Returns an error if the HTTP request fails.
async fn fetch_single(
    client: &HttpClient,
    method: http::Method,
    url: &str,
    headers: &HashMap<String, String>,
    body: Option<&str>,
    extract_next_data: bool,
    scraper_type: ScraperType,
) -> Result<FetchedResponse> {
    match scraper_type {
        ScraperType::Json => {
            if extract_next_data {
                let json = client
                    .get_next_data_json_for_request(method, url, headers, body)
                    .await?;
                Ok(FetchedResponse::Json(json))
            } else {
                let json = client
                    .get_json_for_request(method, url, headers, body)
                    .await?;
                Ok(FetchedResponse::Json(json))
            }
        }
        ScraperType::Html => {
            let html = client
                .query_http_for_request(method, url, headers, body)
                .await?;
            Ok(FetchedResponse::Html(html))
        }
        ScraperType::Text => {
            let text = client
                .query_http_for_request(method, url, headers, body)
                .await?;
            Ok(FetchedResponse::Text(text))
        }
        ScraperType::Static => Ok(FetchedResponse::Static),
    }
}

/// Fetched response shape used by the unified executor.
///
/// Represents the result of a single HTTP request, parsed according to the scraper type.
pub enum FetchedResponse {
    /// Raw HTML body for HTML queries.
    ///
    /// Contains the HTML document as a string.
    Html(String),

    /// Parsed JSON value for JSON queries.
    ///
    /// Contains the parsed JSON data as a `serde_json::Value`.
    Json(Value),

    /// Raw text body for text queries.
    ///
    /// Contains the text payload as a string, split later by row/field delimiters.
    Text(String),

    /// No response (static queries).
    ///
    /// Static queries don't make HTTP requests; they use YAML-declared data.
    Static,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Executes a query polymorphically and returns the individual row items.
///
/// This is the primary entry point used by
/// [`ScraperQueryDefinition::execute_query`](crate::scrapyfy::scraper_query_collection::ScraperQueryDefinition::execute_query).
/// Each returned `HashMap` represents the children of one [`ScraperDataNode`]
/// item extracted from a single row, with sub-queries and post-processes already
/// applied.
///
/// The dispatch path covers:
/// 1. Resolve the request URL by formatting the template with `params`.
/// 2. Resolve request headers and body from the query configuration.
/// 3. Fetch the response(s) in parallel using the configured HTTP client.
/// 4. For each response, extract rows via [`RowLocator`] (CSS selector, JSON
///    pointer, or single row).
/// 5. For each row, build an item by applying entries, then recursively run
///    entry-level sub-queries and sibling sub-queries.
/// 6. Run post-processes on each item and apply `fields_filters`.
///
/// # Arguments
///
/// * `query` - The scraper query to execute (any concrete type behind `&dyn`).
/// * `context` - Runtime context forwarded to sub-query execution.
///
/// # Returns
///
/// A vector containing a single HashMap with all extracted data merged together.
/// Per-row fields (e.g. episodes) accumulate into arrays, while first-occurrence
/// scalar fields (marked with select: first) are kept once.
///
/// # Errors
///
/// Returns an error if the URL cannot be resolved, the HTTP call fails, the
/// response cannot be parsed, or any post-process / sub-query step fails.
pub async fn execute_query_items(
    query: &dyn ScraperQuery,
    context: &QueryContext<'_>,
) -> Result<Vec<HashMap<String, ScraperDataNode>>> {
    let result = execute_query_internal(query, context).await;
    if let Some(url) = resolve_request_urls(query, context).into_iter().next() {
        context
            .http_client
            .close_browser_session_for_url(&url)
            .await?;
    }
    let items = result?;

    // Merge all items into a single root so that per-row fields
    // (e.g. episodes) accumulate into one array, while first-occurrence
    // scalar fields (marked with select: first) are kept once.
    let mut root = ScraperDataNode::default();
    for item in items {
        root.merge(item);
    }

    Ok(vec![root.children])
}

/// Executes a query and returns the items as owned [`ScraperDataNode`] values
/// (not yet unwrapped into their `.children` maps).
///
/// Used internally by [`execute_query_items`] and recursively by sibling /
/// entry-level sub-queries.
///
/// This is the core execution function that:
/// 1. Resolves request URLs
/// 2. Fetches responses in parallel
/// 3. Extracts items from responses
/// 4. Executes entry-level and sibling sub-queries
/// 5. Applies post-processes
/// 6. Applies field filtering
///
/// # Arguments
///
/// * `query` - The scraper query to execute.
/// * `context` - Runtime context containing parameters, request URL, etc.
///
/// # Returns
///
/// A vector of [`ScraperDataNode`] items extracted from the query.
///
/// # Errors
///
/// Returns an error if any step in the execution pipeline fails.
async fn execute_query_internal(
    query: &dyn ScraperQuery,
    context: &QueryContext<'_>,
) -> Result<Vec<ScraperDataNode>> {
    // 1. Handle static queries that have no query_url or request_pointer —
    //    they don't need resolving, fetching, or any response data.
    if query.scraper_type() == ScraperType::Static {
        let items = extract_items(query, &FetchedResponse::Static, "", context)?;
        for mut item in items {
            apply_post_processes(
                query.post_processes(),
                &mut item,
                context.params,
                "",
                None,
                context.http_client,
            )
            .await?;
            if context.fields_filters.is_none()
                || !query_helpers::is_root_filtered(&item, context.fields_filters.unwrap())
            {
                return Ok(vec![item]);
            }
        }
        return Ok(Vec::new());
    }

    // 2. Resolve request URL(s).
    let request_urls = resolve_request_urls(query, context);

    // 2a. Check for input_html mode: when the query provides an input_html
    //     template that resolves to a non-empty string, use it as the response
    //     body instead of fetching.
    let input_html_resolved: Option<String> = query.input_html().and_then(|template| {
        if template.is_empty() {
            return None;
        }
        query_helpers::format_query_template(query.base_url(), template, context.params)
            .ok()
            .filter(|s| !s.is_empty())
    });
    let has_input_html = input_html_resolved.is_some();

    if request_urls.is_empty() && !has_input_html {
        return Ok(Vec::new());
    }

    // 2b. Fetch or use provided input_html.
    let responses: Vec<(String, FetchedResponse)> = if let Some(html_body) = input_html_resolved {
        let ctx_url = request_urls
            .first()
            .cloned()
            .or_else(|| context.params.get("url").cloned())
            .unwrap_or_default();
        vec![(ctx_url, FetchedResponse::Html(html_body))]
    } else {
        fetch_responses(query, &request_urls, context).await?
    };

    // 3. Build the merged root from items.
    let mut all_items = Vec::new();
    for (request_url, response) in responses {
        // a. Extract items and their raw JSON row values from the response.
        //    The raw row values are needed as parent_response for sibling sub-queries
        //    that use context_pointer (e.g. context_pointer: /widgets/* must be
        //    resolved against the row at row_pointer: /data, not the full response).
        let row_values: Vec<(ScraperDataNode, Option<&Value>)> =
            extract_items_with_rows(query, &response, &request_url, context)?;

        for (mut item, row_value) in row_values {
            // b. Execute entry-level sub-queries (recursion).
            execute_entry_sub_queries(query, &mut item, &request_url, context).await?;

            // c. Execute sibling sub-queries (recursion via Box::pin).
            for sibling in query.sub_queries() {
                // Check if this sibling sub-query has a context_pointer, which
                // means it needs to iterate over context rows from the parent
                // row, apply filters, and resolve request URLs per context.
                if let Some(cp) = sibling.context_pointer() {
                    if !cp.is_empty() {
                        // Use the row value as parent for context_pointer resolution,
                        // not the full response. This matches the legacy behavior
                        // where context_pointer is scoped to the extracted row.
                        let parent_json = row_value.or_else(|| match &response {
                            FetchedResponse::Json(v) => Some(v),
                            _ => None,
                        });
                        if let Some(parent_json) = parent_json {
                            // Iterate over context rows selected by context_pointer.
                            let context_values: Vec<&Value> =
                                select_json_values(parent_json, Some(cp), sibling.context_select());
                            // If sibling has a target (e.g. "banners"), each context
                            // becomes a separate array item.
                            let target_path: Vec<&str> = sibling
                                .target()
                                .map(|t| {
                                    t.split('>')
                                        .map(str::trim)
                                        .filter(|s| !s.is_empty())
                                        .collect()
                                })
                                .unwrap_or_default();
                            let use_array_items = !target_path.is_empty();
                            let mut merged = ScraperDataNode::default();
                            for ctx in context_values {
                                let context_item = extract_json_context_entries(
                                    sibling,
                                    ctx,
                                    context,
                                    &request_url,
                                );
                                // Apply filters on context rows.
                                let filters = sibling.filters();
                                let mut filtered_out = false;
                                for (field, allowed) in filters {
                                    let field_values: Vec<String> = select_json_values(
                                        ctx,
                                        Some(field),
                                        HtmlScraperSelectMode::All,
                                    )
                                    .into_iter()
                                    .flat_map(json_value_to_strings)
                                    .collect();
                                    if !field_values.iter().any(|v| allowed.contains(v)) {
                                        filtered_out = true;
                                        break;
                                    }
                                }
                                if filtered_out {
                                    continue;
                                }
                                // Collect results for this context row.
                                let mut ctx_results = Vec::new();

                                if sibling.request_pointer().is_some() {
                                    let sub_context = QueryContext {
                                        params: context.params,
                                        request_url: &request_url,
                                        response_body: context.response_body,
                                        http_client: context.http_client,
                                        fields_filters: None,
                                        parent_response: Some(ctx),
                                    };
                                    let sub_items =
                                        Box::pin(execute_query_internal(sibling, &sub_context))
                                            .await?;
                                    ctx_results = sub_items;
                                } else if !sibling.request_actions().is_empty() {
                                    let urls: Vec<String> =
                                        select_json_values(ctx, None, HtmlScraperSelectMode::All)
                                            .into_iter()
                                            .flat_map(json_value_to_strings)
                                            .collect();
                                    if !urls.is_empty() {
                                        let mut processed: Vec<String> = Vec::new();
                                        for url in urls {
                                            let mut vals = vec![url];
                                            for action in sibling.request_actions() {
                                                vals = action.apply(
                                                    &None,
                                                    vals,
                                                    context.params,
                                                    context.request_url,
                                                    None,
                                                    None,
                                                );
                                            }
                                            processed.extend(vals);
                                        }
                                        for url in processed {
                                            if url.is_empty() {
                                                continue;
                                            }
                                            let method = sibling.request_method().as_http_method();
                                            let headers = resolve_request_headers(
                                                sibling.request_headers(),
                                                context.params,
                                                &url,
                                                Some(ctx),
                                            );
                                            let body = resolve_request_body(
                                                sibling.request_body_pointer(),
                                                sibling.request_body_select(),
                                                sibling.request_body_actions(),
                                                context.params,
                                                &url,
                                                Some(ctx),
                                            );
                                            let client = context
                                                .http_client
                                                .configured(sibling.http_config().clone());
                                            let response = fetch_single(
                                                &client,
                                                method,
                                                &url,
                                                &headers,
                                                body.as_deref(),
                                                sibling.extract_next_data(),
                                                sibling.scraper_type(),
                                            )
                                            .await?;
                                            let fetched_json = match &response {
                                                FetchedResponse::Json(v) => Some(v),
                                                _ => None,
                                            };
                                            let sibling_rows = extract_items_with_rows(
                                                sibling, &response, &url, context,
                                            )?;
                                            // Execute children sub-queries instead of re-executing the
                                            // sibling itself. The sibling had a context_pointer that was
                                            // already resolved to produce the fetch URL — re-executing
                                            // the same sibling with the fetched response would try to
                                            // resolve the context_pointer again against a response that
                                            // doesn't contain it, producing empty results.
                                            let mut merged_sub = ScraperDataNode::default();
                                            if sibling_rows.is_empty() {
                                                for child_sibling in sibling.sub_queries() {
                                                    let child_context = QueryContext {
                                                        params: context.params,
                                                        request_url: &url,
                                                        response_body: None,
                                                        http_client: context.http_client,
                                                        fields_filters: None,
                                                        parent_response: fetched_json,
                                                    };
                                                    let child_items =
                                                        Box::pin(execute_query_internal(
                                                            child_sibling,
                                                            &child_context,
                                                        ))
                                                        .await?;
                                                    for child_item in child_items {
                                                        merged_sub.merge(child_item);
                                                    }
                                                }
                                            } else {
                                                for (mut row_item, row_value) in sibling_rows {
                                                    for child_sibling in sibling.sub_queries() {
                                                        let child_context = QueryContext {
                                                            params: context.params,
                                                            request_url: &url,
                                                            response_body: None,
                                                            http_client: context.http_client,
                                                            fields_filters: None,
                                                            parent_response: row_value
                                                                .or(fetched_json),
                                                        };
                                                        let child_items =
                                                            Box::pin(execute_query_internal(
                                                                child_sibling,
                                                                &child_context,
                                                            ))
                                                            .await?;
                                                        for child_item in child_items {
                                                            row_item.merge(child_item);
                                                        }
                                                    }
                                                    merged_sub.merge(row_item);
                                                }
                                            }
                                            // Merge sub-query results directly into the current item
                                            // so fields like `video` are added to the same banner
                                            // entry rather than creating a separate array item.
                                            ctx_results.push(merged_sub);
                                        }
                                    }
                                }

                                // Accumulate results for this context row.
                                if use_array_items {
                                    let target = walk_mut(&mut item, &target_path);
                                    for result in ctx_results {
                                        let mut combined = context_item.clone();
                                        combined.merge(result);
                                        target.items.push(combined);
                                    }
                                } else {
                                    for result in ctx_results {
                                        let mut combined = context_item.clone();
                                        combined.merge(result);
                                        merged.merge(combined);
                                    }
                                }
                            }
                            if !use_array_items {
                                merge_targeted(&mut item, merged, sibling.target());
                            }
                            continue; // Skip the generic path below.
                        }
                    }
                }
                // Default: generic unified execution path (for sub-queries without
                // context_pointer).
                let parent_json = row_value.or_else(|| match &response {
                    FetchedResponse::Json(v) => Some(v),
                    _ => None,
                });
                let sub_context = QueryContext {
                    params: context.params,
                    request_url: &request_url,
                    response_body: context.response_body,
                    http_client: context.http_client,
                    fields_filters: None,
                    parent_response: parent_json,
                };
                let sibling_root = Box::pin(execute_query_internal(sibling, &sub_context)).await?;
                merge_targeted_items(&mut item, sibling_root, sibling.target());
            }

            // d. Apply post-processes on this item.
            let response_body = response_body_str(&response);
            apply_post_processes(
                query.post_processes(),
                &mut item,
                context.params,
                &request_url,
                response_body,
                context.http_client,
            )
            .await?;

            // e. Apply field filtering.
            if context.fields_filters.is_none()
                || !query_helpers::is_root_filtered(&item, context.fields_filters.unwrap())
            {
                all_items.push(item);
            }
        }
    }

    Ok(all_items)
}

// ---------------------------------------------------------------------------
// Downcast helpers
// ---------------------------------------------------------------------------

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
fn as_html_entry(entry: &dyn ScraperEntrySpec) -> Option<&HtmlScraperEntry> {
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
fn as_json_entry(entry: &dyn ScraperEntrySpec) -> Option<&JsonScraperEntry> {
    entry.as_any().downcast_ref::<JsonScraperEntry>()
}

/// Extracts query-level context fields from the current JSON context row.
fn extract_json_context_entries(
    query: &dyn ScraperQuery,
    row: &Value,
    context: &QueryContext<'_>,
    request_url: &str,
) -> ScraperDataNode {
    let mut item = ScraperDataNode::default();
    for entry in query.context_entries() {
        if let Some(json_entry) = as_json_entry(entry) {
            json_entry.apply_to(&mut item, row, context.params, request_url);
        }
    }
    item
}

/// Returns JSON row filters for a concrete JSON query or sub-query.
fn json_row_filters(query: &dyn ScraperQuery) -> Option<&HashMap<String, Vec<String>>> {
    if let Some(json_query) = query.as_any().downcast_ref::<JsonScraperQuery>() {
        return Some(&json_query.row_filters);
    }

    query
        .as_any()
        .downcast_ref::<JsonScraperSubQuery>()
        .map(|json_query| &json_query.row_filters)
}

// ---------------------------------------------------------------------------
// Request resolution
// ---------------------------------------------------------------------------

/// Resolves the request URL by formatting the template with resolved params.
///
/// The URL template is resolved using [`query_helpers::format_query_template`]
/// with the parameters from the context. `request_url_actions`, when present,
/// transform the resolved template before the HTTP call.
///
/// # Arguments
///
/// * `query` - The query containing the URL template.
/// * `context` - Runtime context containing parameters for template resolution.
/// * `response` - Optional parent response for sub-query URL resolution.
///
/// # Returns
///
/// A vector containing the resolved URL(s). Returns empty vector for sub-queries
/// without a query_url template when response/context is not provided.
fn resolve_request_urls(query: &dyn ScraperQuery, context: &QueryContext<'_>) -> Vec<String> {
    // First, try to resolve from query_url template (root queries)
    if !query.query_url().is_empty() {
        let url = query_helpers::format_query_template(
            query.base_url(),
            query.query_url(),
            context.params,
        )
        .unwrap_or_else(|_| query.query_url().to_string());
        let mut urls = vec![url];
        for action in query.request_url_actions() {
            urls = action.apply(&None, urls, context.params, context.request_url, None, None);
        }
        return urls;
    }

    if let Some(parent_response) = context.parent_response {
        if let Some(rp) = query.request_pointer() {
            if !rp.is_empty() {
                let values: Vec<String> =
                    select_json_values(parent_response, Some(rp), query.request_select())
                        .into_iter()
                        .flat_map(json_value_to_strings)
                        .collect();
                if !values.is_empty() {
                    let mut resolved_urls = Vec::new();
                    for value in values {
                        let mut url_values = vec![value];
                        for action in query.request_actions() {
                            url_values = action.apply(
                                &None,
                                url_values,
                                context.params,
                                context.request_url,
                                None,
                                None,
                            );
                        }
                        if let Some(url) = url_values.into_iter().find(|v| !v.is_empty()) {
                            resolved_urls.push(url);
                        }
                    }
                    return resolved_urls;
                }
            }
        } else if !query.request_actions().is_empty() {
            // No request_pointer but request_actions present — use the parent
            // response value directly as the input for the action pipeline.
            // This covers the PROMOBOX sub-chain pattern where
            // context_pointer: /mediaId selects a value and
            // request_actions: format_text builds the full URL.
            let values: Vec<String> =
                select_json_values(parent_response, None, query.request_select())
                    .into_iter()
                    .flat_map(json_value_to_strings)
                    .collect();
            if !values.is_empty() {
                let mut resolved_urls = Vec::new();
                for value in values {
                    let mut url_values = vec![value];
                    for action in query.request_actions() {
                        url_values = action.apply(
                            &None,
                            url_values,
                            context.params,
                            context.request_url,
                            None,
                            None,
                        );
                    }
                    for url in url_values {
                        if !url.is_empty() {
                            resolved_urls.push(url);
                        }
                    }
                }
                return resolved_urls;
            }
        }
    }
    vec![]
}

/// Resolves request headers from the query's configured header list.
///
/// Each header's value is resolved from the params context using the
/// header's pointer and actions pipeline.
///
/// # Arguments
///
/// * `headers` - The list of configured headers to resolve.
/// * `params` - Runtime template parameters.
/// * `request_url` - URL of the parent request for action resolution.
/// * `parent_response` - Optional JSON response used for pointer resolution.
///
/// # Returns
///
/// A hash map of header names to their resolved values.
fn resolve_request_headers(
    headers: &[crate::scrapyfy::scraper_json::query::ScraperRequestHeader],
    params: &HashMap<String, String>,
    request_url: &str,
    parent_response: Option<&Value>,
) -> HashMap<String, String> {
    let request_context = query_helpers::build_params_json_value(params);
    let request_context = parent_response.unwrap_or(&request_context);
    let mut resolved = HashMap::new();
    for header in headers {
        if let Some((name, value)) = header.resolve(request_context, params, request_url) {
            resolved.insert(name, value);
        }
    }
    resolved
}

/// Resolves the optional request body from the query's body pointer/actions.
///
/// Selects JSON values from the parent response when available, otherwise from
/// the params context, then applies the body actions pipeline.
///
/// # Arguments
///
/// * `pointer` - Optional JSON pointer to select the body value.
/// * `select` - Selection mode for the pointer (First or All).
/// * `actions` - Actions to apply to the selected values.
/// * `params` - Runtime template parameters.
/// * `request_url` - URL of the parent request for action resolution.
/// * `parent_response` - Optional JSON response used for pointer resolution.
///
/// # Returns
///
/// The resolved request body as a string, or `None` if no body is configured
/// or if the resolved value is empty.
fn resolve_request_body(
    pointer: Option<&str>,
    select: HtmlScraperSelectMode,
    actions: &[crate::scrapyfy::ScraperAction],
    params: &HashMap<String, String>,
    request_url: &str,
    parent_response: Option<&Value>,
) -> Option<String> {
    if pointer.is_none() && actions.is_empty() {
        return None;
    }
    let request_context = query_helpers::build_params_json_value(params);
    let request_context = parent_response.unwrap_or(&request_context);
    let mut values: Vec<String> = select_json_values(request_context, pointer, select)
        .into_iter()
        .flat_map(crate::scrapyfy::scraper_json::entry::json_value_to_strings)
        .collect();
    for action in actions {
        values = action.apply(&None, values, params, request_url, None, None);
    }
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

// ---------------------------------------------------------------------------
// Fetch
// ---------------------------------------------------------------------------

/// Fetches all responses in parallel using the configured HTTP client.
///
/// Executes HTTP requests for all URLs concurrently, using the configured
/// concurrency level ([`DEFAULT_FETCH_CONCURRENCY`]).
///
/// # Arguments
///
/// * `query` - The query being executed.
/// * `urls` - The URLs to fetch.
/// * `context` - Runtime context containing parameters and HTTP client.
///
/// # Returns
///
/// A vector of tuples containing the URL and its corresponding [`FetchedResponse`].
/// Results are ordered according to the input URLs.
///
/// # Errors
///
/// Returns an error if any HTTP request fails.
async fn fetch_responses(
    query: &dyn ScraperQuery,
    urls: &[String],
    context: &QueryContext<'_>,
) -> Result<Vec<(String, FetchedResponse)>> {
    let method = query.request_method().as_http_method();
    let headers = resolve_request_headers(
        query.request_headers(),
        context.params,
        urls.first().map(String::as_str).unwrap_or(""),
        context.parent_response,
    );
    let body = resolve_request_body(
        query.request_body_pointer(),
        query.request_body_select(),
        query.request_body_actions(),
        context.params,
        urls.first().map(String::as_str).unwrap_or(""),
        context.parent_response,
    );
    let extract_next_data = query.extract_next_data();
    let scraper_type = query.scraper_type();
    let execution = query.http_config().execution;
    match execution {
        Some(ScraperHttpExecution::PageNavigate) => {
            if scraper_type != ScraperType::Html {
                anyhow::bail!("page_navigate is only supported by HTML queries");
            }
            if query.http_config().browser_context != Some(ScraperBrowserContext::Origin) {
                anyhow::bail!("page_navigate requires http.browser_context: origin");
            }
            if method != http::Method::GET {
                anyhow::bail!("page_navigate only supports GET queries");
            }
        }
        Some(ScraperHttpExecution::PageFetch) => {
            anyhow::bail!("page_fetch is only supported by HTML sub-queries");
        }
        Some(ScraperHttpExecution::PageClick) => {
            anyhow::bail!("page_click is only supported by HTML sub-queries");
        }
        Some(ScraperHttpExecution::Direct) | None => {}
    }
    // Configure the client with the query's HTTP settings once, then clone
    // for each parallel job.
    let configured_client = context.http_client.configured(query.http_config().clone());

    // Filter out empty URLs before fetching.
    let non_empty_urls: Vec<String> = urls
        .iter()
        .filter(|url| !url.trim().is_empty())
        .cloned()
        .collect();

    let jobs = non_empty_urls
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, url)| {
            let headers = headers.clone();
            let body = body.clone();
            let method = method.clone();
            let client = configured_client.clone();
            async move {
                let response = if execution == Some(ScraperHttpExecution::PageNavigate) {
                    FetchedResponse::Html(client.page_navigate_for_request(&url, &headers).await?)
                } else {
                    fetch_single(
                        &client,
                        method,
                        &url,
                        &headers,
                        body.as_deref(),
                        extract_next_data,
                        scraper_type,
                    )
                    .await?
                };
                Ok::<(usize, (String, FetchedResponse)), anyhow::Error>((index, (url, response)))
            }
        });

    let results: Vec<Result<(usize, (String, FetchedResponse))>> = stream::iter(jobs)
        .buffer_unordered(DEFAULT_FETCH_CONCURRENCY)
        .collect()
        .await;

    let mut ordered = Vec::with_capacity(results.len());
    for result in results {
        ordered.push(result?);
    }
    ordered.sort_by_key(|(index, _)| *index);
    Ok(ordered.into_iter().map(|(_, value)| value).collect())
}

// ---------------------------------------------------------------------------
// Row extraction
// ---------------------------------------------------------------------------

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
fn extract_items(
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
                            request_url,
                            html_body,
                        );
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
                        json_entry.apply_to(&mut item, row_value, context.params, request_url);
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
                    static_entry.apply_to(&mut item, context.params)?;
                }
            }
            vec![item]
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
                        text_entry.apply_to(&mut item, &fields, context.params, request_url);
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
fn extract_items_with_rows<'a>(
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
                        json_entry.apply_to(&mut item, row_value, context.params, request_url);
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
// ---------------------------------------------------------------------------
// Entry sub-queries
// ---------------------------------------------------------------------------

/// Executes sub-queries attached to entries of the given query.
///
/// For each entry, collects the produced values from the item and runs every
/// attached sub-query. The sub-query result is merged into the item.
///
/// For entry-level sub-queries whose `request_pointer` is `None` or `"parent"`,
/// the entry's extracted values are used as the fetch URL(s).  The sub-query
/// response is parsed according to its scraper type, rows are extracted via
/// its [`RowLocator`], and the sub-query entries are applied to each row.
///
/// # Arguments
///
/// * `query` - The query being executed.
/// * `item` - The item being populated with extracted data.
/// * `request_url` - The URL of the request that produced the parent response.
/// * `context` - Runtime context containing parameters.
///
/// # Returns
///
/// `Ok(())` on success.
///
/// # Errors
///
/// Returns an error if any sub-query execution fails.
async fn execute_entry_sub_queries(
    query: &dyn ScraperQuery,
    item: &mut ScraperDataNode,
    request_url: &str,
    context: &QueryContext<'_>,
) -> Result<()> {
    let entries = query.entries();
    execute_entry_sub_queries_for_entries(&entries, item, request_url, context).await
}

/// Executes entry-level sub-queries for one output node and its nested groups.
///
/// Each object-group item receives its own template parameters, preventing
/// follow-up requests from mixing values extracted from distinct rows.
async fn execute_entry_sub_queries_for_entries(
    entries: &[&dyn ScraperEntrySpec],
    item: &mut ScraperDataNode,
    request_url: &str,
    context: &QueryContext<'_>,
) -> Result<()> {
    // Collect entry names → values from the item so we can map sub-query
    // `request_pointer` to the entry value that seeds the URL.
    let entry_values: HashMap<String, Vec<String>> = collect_entry_values(item);
    let sub_query_params = build_sub_query_params(context.params, &entry_values);

    for entry in entries {
        // Determine which values from this entry seed sub-query URLs.
        let entry_name = entry.name().to_string();
        let parent_urls = entry_values.get(&entry_name).cloned().unwrap_or_default();
        let has_parent_urls = !parent_urls.is_empty();

        for sub_query in entry.sub_queries() {
            if has_parent_urls {
                // Entry-level sub-query: use the entry values as fetch URL(s).
                let rp = sub_query.request_pointer();
                if rp.is_none() || rp == Some(PARENT_SENTINEL) {
                    // Fetch each non-empty value as a URL, parse, extract, merge.
                    // Skip empty URLs, URLs with unresolved placeholders ({}),
                    // and URLs with empty query parameter values (e.g. ?assetId=)
                    // to avoid sending invalid requests to remote servers.
                    let mut merged = ScraperDataNode::default();
                    for url in &parent_urls {
                        let trimmed = url.trim();
                        if trimmed.is_empty()
                            || trimmed.contains("{}")
                            || !is_valid_fetch_url(trimmed)
                        {
                            continue;
                        }
                        let mut item_clone = ScraperDataNode::default();
                        let _ = fetch_and_extract_for_entry_sub_query(
                            sub_query,
                            url,
                            &mut item_clone,
                            context,
                            &sub_query_params,
                        )
                        .await?;
                        merged.merge(item_clone);
                    }
                    // Clear the parent entry's values before merging the
                    // sub-query result — the sub-query *replaces* the URL
                    // with the decoded embed link(s).  Use the entry's
                    // name path for targeted merging.
                    let path: Vec<&str> = entry_name
                        .split('>')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .collect();
                    if path.is_empty() {
                        item.merge(merged);
                    } else {
                        // Replace the target node: walk the path, clear it,
                        // then inject the sub-query's leaf values directly
                        // as individual items (array elements) that survive
                        // `keep_first_values`.  Using `values` would cause
                        // truncation to 1 element in `execute_query_items`.
                        let target = walk_mut(item, &path);
                        target.values.clear();
                        target.children.clear();
                        target.items.clear();
                        let leaf_name = path.last().copied().unwrap_or_default();
                        if let Some(child) = merged.children.get(leaf_name) {
                            if merged.children.len() == 1
                                && child.children.is_empty()
                                && ((!child.items.is_empty()
                                    && child.items.iter().all(|item| {
                                        item.children.is_empty() && item.items.is_empty()
                                    }))
                                    || !child.values.is_empty())
                            {
                                let scalar_values: Vec<String> = if !child.values.is_empty() {
                                    child.values.clone()
                                } else {
                                    child
                                        .items
                                        .iter()
                                        .flat_map(|item| item.values.clone())
                                        .collect()
                                };
                                if target.output_type.is_some_and(ScraperOutputType::is_scalar) {
                                    target.values = scalar_values.into_iter().take(1).collect();
                                } else {
                                    for val in scalar_values {
                                        let entry = scalar_node_from_source_value(val, child);
                                        target.items.push(entry);
                                    }
                                }
                                continue;
                            }
                        }
                        // Convert fetched rows into array items — each
                        // sibling field (e.g. embed-link and name) is
                        // zipped by index so that values at the same
                        // position become a single item.
                        //
                        // Algorithm:
                        // 1. Collect all children that have the same
                        //    number of values and zip them together by
                        //    index, producing one item per index with
                        //    all fields combined.
                        // 2. Any child with a different cardinality is
                        //    left as individual items (legacy fallback).
                        // 3. Root-level `merged.values` are appended
                        //    as individual items.
                        if merged.children.len() >= 2 {
                            // Determine the target cardinality from
                            // the first child that has values.
                            let cardinality = merged
                                .children
                                .values()
                                .find_map(|c| {
                                    if !c.values.is_empty() {
                                        Some(c.values.len())
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or(0);
                            if cardinality > 0 {
                                // Partition children: those matching the
                                // cardinality are zipped; others are left
                                // as individual items.
                                let mut common: Vec<(&String, &Vec<String>)> = Vec::new();
                                let mut leftover: Vec<(&String, &ScraperDataNode)> = Vec::new();
                                for (name, child) in &merged.children {
                                    if child.values.len() == cardinality {
                                        common.push((name, &child.values));
                                    } else if !child.values.is_empty() {
                                        leftover.push((name, child));
                                    }
                                }
                                if common.len() >= 2 {
                                    // Zip common fields by index.
                                    for i in 0..cardinality {
                                        let mut item_node = ScraperDataNode::default();
                                        for (name, values) in &common {
                                            item_node.children.insert(
                                                (*name).clone(),
                                                scalar_node_from_source_value(
                                                    values[i].clone(),
                                                    merged.children.get(*name).unwrap(),
                                                ),
                                            );
                                        }
                                        target.items.push(item_node);
                                    }
                                    // Add leftover children as individual items.
                                    for (name, child) in leftover {
                                        for val in &child.values {
                                            let mut item_node = ScraperDataNode::default();
                                            item_node.children.insert(
                                                (*name).clone(),
                                                scalar_node_from_source_value(val.clone(), child),
                                            );
                                            target.items.push(item_node);
                                        }
                                    }
                                } else {
                                    // Not enough common fields — fall
                                    // back to individual-item logic.
                                    for (name, child) in &merged.children {
                                        for val in &child.values {
                                            let mut item_node = ScraperDataNode::default();
                                            item_node.children.insert(
                                                (*name).clone(),
                                                scalar_node_from_source_value(val.clone(), child),
                                            );
                                            target.items.push(item_node);
                                        }
                                    }
                                    for val in &merged.values {
                                        let entry = ScraperDataNode::from_values_typed(
                                            vec![val.clone()],
                                            ScraperOutputType::String,
                                        );
                                        target.items.push(entry);
                                    }
                                }
                            } else {
                                // No values — fall back to individual-item logic.
                                for (name, child) in &merged.children {
                                    for val in &child.values {
                                        let mut item_node = ScraperDataNode::default();
                                        item_node.children.insert(
                                            (*name).clone(),
                                            scalar_node_from_source_value(val.clone(), child),
                                        );
                                        target.items.push(item_node);
                                    }
                                }
                                for val in &merged.values {
                                    let entry = ScraperDataNode::from_values_typed(
                                        vec![val.clone()],
                                        ScraperOutputType::String,
                                    );
                                    target.items.push(entry);
                                }
                            }
                        } else {
                            // Single child — use the original
                            // individual-item logic.
                            for (name, child) in &merged.children {
                                for val in &child.values {
                                    let mut item_node = ScraperDataNode::default();
                                    item_node.children.insert(
                                        name.clone(),
                                        scalar_node_from_source_value(val.clone(), child),
                                    );
                                    target.items.push(item_node);
                                }
                            }
                            for val in &merged.values {
                                let entry = ScraperDataNode::from_values_typed(
                                    vec![val.clone()],
                                    ScraperOutputType::String,
                                );
                                target.items.push(entry);
                            }
                        }
                    }
                } else {
                    // Non-default request_pointer → fall through to generic executor.
                    let sub_context = QueryContext {
                        params: context.params,
                        request_url,
                        response_body: context.response_body,
                        http_client: context.http_client,
                        fields_filters: None,
                        parent_response: None,
                    };
                    let sub_items =
                        Box::pin(execute_query_internal(sub_query, &sub_context)).await?;
                    merge_targeted_items(item, sub_items, sub_query.target());
                }
            } else {
                // No parent values available — try the generic executor path.
                let sub_context = QueryContext {
                    params: context.params,
                    request_url,
                    response_body: context.response_body,
                    http_client: context.http_client,
                    fields_filters: None,
                    parent_response: None,
                };
                let sub_items = Box::pin(execute_query_internal(sub_query, &sub_context)).await?;
                merge_targeted_items(item, sub_items, sub_query.target());
            }
        }

        let child_entries = entry.sub_entries();
        if child_entries.is_empty() {
            continue;
        }
        let path: Vec<&str> = entry
            .name()
            .split('>')
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .collect();
        let Some(group) = get_node_mut(item, &path) else {
            continue;
        };
        for group_item in &mut group.items {
            Box::pin(execute_entry_sub_queries_for_entries(
                &child_entries,
                group_item,
                request_url,
                context,
            ))
            .await?;
        }
    }
    Ok(())
}

/// Collects all leaf entry values from a [`ScraperDataNode`] item into a
/// `{entry_name → [value, …]}` map.
///
/// This is used by [`execute_entry_sub_queries`] to map entry names to their
/// extracted scalar values so they can seed sub-query fetch URLs.
///
/// # Arguments
///
/// * `item` - The item from which to collect leaf entry values.
fn collect_entry_values(item: &ScraperDataNode) -> HashMap<String, Vec<String>> {
    let mut map = HashMap::new();
    collect_entry_values_recursive(item, &mut map, empty_path());
    map
}

/// Adds current entry values to the template context of an entry sub-query.
///
/// Both the original field name and an underscore-normalized variant are
/// available, so YAML can reference a field such as `player-id` through the
/// placeholder `{player_id}` accepted by the template grammar.
fn build_sub_query_params(
    base: &HashMap<String, String>,
    entry_values: &HashMap<String, Vec<String>>,
) -> HashMap<String, String> {
    let mut params = base.clone();
    for (name, values) in entry_values {
        let Some(value) = values.first().filter(|value| !value.is_empty()) else {
            continue;
        };
        params.insert(name.clone(), value.clone());
        let normalized = name
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || character == '_' {
                    character
                } else {
                    '_'
                }
            })
            .collect::<String>();
        if !normalized.is_empty() {
            params.insert(normalized, value.clone());
        }
    }
    params
}

/// Recursive helper for [`collect_entry_values`].
///
/// Walks the tree accumulating leaf values under their `>`-separated path.
fn collect_entry_values_recursive<'a>(
    node: &'a ScraperDataNode,
    map: &mut HashMap<String, Vec<String>>,
    path: &[&str],
) {
    // Recurse into children first.
    for (name, child) in &node.children {
        let mut child_path = path.to_vec();
        child_path.push(name);
        collect_entry_values_recursive(child, map, &child_path);
    }
    // Process items (group entries).
    for item in &node.items {
        collect_entry_values_recursive(item, map, empty_path());
    }
    // Leaf values: append to the path entry.
    if !node.values.is_empty() {
        let key = if path.is_empty() {
            String::new()
        } else {
            path.join(" > ")
        };
        map.entry(key).or_default().extend(node.values.clone());
    }
}

/// Fetches a URL on behalf of an entry-level sub-query, parses the response
/// according to the sub-query's [`ScraperType`], extracts rows via its
/// [`RowLocator`], and applies the sub-query entries to every row.
///
/// Headers from the sub-query configuration are resolved and sent with the
/// request.
///
/// # Arguments
///
/// * `sub_query` - The entry-level sub-query to execute.
/// * `url` - The URL to fetch (derived from the parent entry value).
/// * `item` - The item to populate with extracted data.
/// * `context` - Runtime context containing the HTTP client and params.
///
/// # Returns
///
/// `Ok(())` on success.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or if entry application fails.
async fn fetch_and_extract_for_entry_sub_query(
    sub_query: &dyn ScraperQuery,
    url: &str,
    item: &mut ScraperDataNode,
    context: &QueryContext<'_>,
    params: &HashMap<String, String>,
) -> Result<()> {
    let method = sub_query.request_method().as_http_method();
    let headers: HashMap<String, String> = resolve_request_headers(
        sub_query.request_headers(),
        params,
        url,
        context.parent_response,
    );
    let body: Option<String> = resolve_request_body(
        sub_query.request_body_pointer(),
        sub_query.request_body_select(),
        sub_query.request_body_actions(),
        params,
        url,
        context.parent_response,
    );
    let client = context
        .http_client
        .configured(sub_query.http_config().clone());
    let response = if matches!(
        sub_query.http_config().execution,
        Some(ScraperHttpExecution::PageFetch)
    ) {
        if sub_query.scraper_type() != ScraperType::Html {
            anyhow::bail!("page_fetch is only supported by HTML sub-queries");
        }
        if sub_query.http_config().browser_context != Some(ScraperBrowserContext::Origin) {
            anyhow::bail!("page_fetch requires http.browser_context: origin");
        }
        let page_url_template = sub_query
            .http_config()
            .page_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("page_fetch requires http.page_url"))?;
        let page_url =
            query_helpers::format_query_template(sub_query.base_url(), page_url_template, params)?;
        let token = sub_query.http_config().browser_token.as_ref();
        let retry_once =
            token.is_some_and(|config| config.retry_on_rejection == ScraperBrowserTokenRetry::Once);
        let mut attempts = 0;
        loop {
            match client
                .page_fetch_for_request(
                    &page_url,
                    method.clone(),
                    url,
                    &headers,
                    body.as_deref(),
                    token,
                )
                .await
            {
                Ok(html) => break FetchedResponse::Html(html),
                Err(error)
                    if retry_once
                        && attempts == 0
                        && error
                            .downcast_ref::<arachnea_http::ArachneaHttpError>()
                            .is_some_and(|error| {
                                matches!(
                                    error,
                                    arachnea_http::ArachneaHttpError::TokenRejected { .. }
                                )
                            }) =>
                {
                    attempts += 1;
                }
                Err(error) => return Err(error),
            }
        }
    } else if matches!(
        sub_query.http_config().execution,
        Some(ScraperHttpExecution::PageClick)
    ) {
        if sub_query.scraper_type() != ScraperType::Html {
            anyhow::bail!("page_click is only supported by HTML sub-queries");
        }
        if sub_query.http_config().browser_context != Some(ScraperBrowserContext::Origin) {
            anyhow::bail!("page_click requires http.browser_context: origin");
        }
        let page_url_template = sub_query
            .http_config()
            .page_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("page_click requires http.page_url"))?;
        let click = sub_query
            .http_config()
            .browser_click
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("page_click requires http.browser_click"))?;
        let page_url =
            query_helpers::format_query_template(sub_query.base_url(), page_url_template, params)?;
        let selector =
            query_helpers::format_query_template(sub_query.base_url(), &click.selector, params)?;
        let wait_for_selector = query_helpers::format_query_template(
            sub_query.base_url(),
            &click.wait_for_selector,
            params,
        )?;
        FetchedResponse::Html(
            client
                .page_click_for_request(&page_url, &selector, &wait_for_selector)
                .await?,
        )
    } else {
        fetch_single(
            &client,
            method,
            url,
            &headers,
            body.as_deref(),
            sub_query.extract_next_data(),
            sub_query.scraper_type(),
        )
        .await?
    };
    let mut nested_response_body: Option<String> = None;
    let mut nested_parent_response: Option<Value> = None;

    match response {
        FetchedResponse::Html(html) => {
            let doc = ::scraper::Html::parse_document(&html);
            match sub_query.row_locator() {
                RowLocator::Selector {
                    ref selector,
                    select,
                } => {
                    let sel = match ::scraper::Selector::parse(selector) {
                        Ok(s) => s,
                        Err(_) => return Ok(()),
                    };
                    let select_first = matches!(select, HtmlScraperSelectMode::First);
                    for element in doc.select(&sel) {
                        for entry in sub_query.entries() {
                            if let Some(html_entry) = as_html_entry(entry) {
                                html_entry.apply_to(item, element, params, url, Some(&html));
                            }
                        }
                        if select_first {
                            break;
                        }
                    }
                }
                RowLocator::Single | RowLocator::Pointer(_) => {
                    // No HTML element available for RowLocator::Single or
                    // RowLocator::Pointer — skip HTML entry application.
                }
            }
            nested_response_body = Some(html);
        }
        FetchedResponse::Json(value) => {
            let pointer = match sub_query.row_locator() {
                RowLocator::Pointer(ref p) => p.clone(),
                _ => return Ok(()),
            };
            let rows = select_json_values(&value, Some(&pointer), HtmlScraperSelectMode::All);
            for row_value in rows {
                let mut filtered_out = false;
                if let Some(row_filters) = json_row_filters(sub_query) {
                    for (field, allowed) in row_filters {
                        let field_values: Vec<String> =
                            select_json_values(row_value, Some(field), HtmlScraperSelectMode::All)
                                .into_iter()
                                .flat_map(json_value_to_strings)
                                .collect();
                        if !field_values.iter().any(|v| allowed.contains(v)) {
                            filtered_out = true;
                            break;
                        }
                    }
                }
                if filtered_out {
                    continue;
                }
                for entry in sub_query.entries() {
                    if let Some(json_entry) = as_json_entry(entry) {
                        json_entry.apply_to(item, row_value, params, url);
                    }
                }
            }
            nested_parent_response = Some(value);
        }
        FetchedResponse::Text(_) => {}
        FetchedResponse::Static => {}
    }

    if !sub_query.entries().is_empty() {
        let sub_context = QueryContext {
            params,
            request_url: url,
            response_body: nested_response_body.as_deref(),
            http_client: context.http_client,
            fields_filters: None,
            parent_response: nested_parent_response.as_ref(),
        };
        Box::pin(execute_entry_sub_queries(
            sub_query,
            item,
            url,
            &sub_context,
        ))
        .await?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Post-process
// ---------------------------------------------------------------------------

/// Applies the query's post-process pipeline to a single item.
///
/// Each post-process is executed in sequence, transforming the item.
///
/// # Arguments
///
/// * `post_processes` - The list of post-processes to apply.
/// * `item` - The item to process.
/// * `params` - Runtime template parameters.
/// * `request_url` - The URL of the request that produced the item.
/// * `response_body` - Optional response body for post-processes that need it.
/// * `http_client` - HTTP client for post-processes that make requests.
///
/// # Returns
///
/// `Ok(())` on success.
///
/// # Errors
///
/// Returns an error if any post-process application fails.
async fn apply_post_processes(
    post_processes: &[ScraperPostProcess],
    item: &mut ScraperDataNode,
    params: &HashMap<String, String>,
    request_url: &str,
    response_body: Option<&str>,
    http_client: &HttpClient,
) -> Result<()> {
    let ctx = ScraperPostProcessContext {
        params,
        request_url,
        response_body,
        http_client,
    };
    for post_process in post_processes {
        post_process.apply(item, &ctx).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Merge
// ---------------------------------------------------------------------------

/// Merges a sub-query result into an item, honouring the sub-query's
/// `target` path.
///
/// If a target path is specified, the source data is nested
/// under that path. Otherwise, the source is merged directly into the item.
///
/// # Arguments
///
/// * `item` - The item to merge the source into.
/// * `source` - The source data to merge.
/// * `target` - Optional target path for nesting.
fn merge_targeted(item: &mut ScraperDataNode, source: ScraperDataNode, target: Option<&str>) {
    if let Some(target) = target {
        let path: Vec<&str> = target
            .split('>')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if path.is_empty() {
            item.merge(source);
        } else {
            item.push_node_typed(&path, source, ScraperOutputType::ObjectArray);
        }
        return;
    }
    item.merge(source);
}

/// Merges sub-query rows into an item, preserving row cardinality when a
/// target path is declared.
///
/// A targeted sub-query represents an object array; each returned row must
/// become one item under the target. Without a target, rows are merged into
/// the current item as before.
fn merge_targeted_items(
    item: &mut ScraperDataNode,
    sources: Vec<ScraperDataNode>,
    target: Option<&str>,
) {
    if let Some(target) = target {
        let path: Vec<&str> = target
            .split('>')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if path.is_empty() {
            for source in sources {
                item.merge(source);
            }
        } else {
            for source in sources {
                item.push_node_typed(&path, source, ScraperOutputType::ObjectArray);
            }
        }
        return;
    }

    let mut merged = ScraperDataNode::default();
    for source in sources {
        merged.merge(source);
    }
    item.merge(merged);
}

fn scalar_output_type_from_source(source: &ScraperDataNode) -> ScraperOutputType {
    source
        .output_type
        .and_then(ScraperOutputType::array_element_type)
        .or(source.output_type)
        .filter(|output_type| output_type.is_scalar())
        .unwrap_or(ScraperOutputType::String)
}

fn scalar_node_from_source_value(value: String, source: &ScraperDataNode) -> ScraperDataNode {
    ScraperDataNode::from_values_typed(vec![value], scalar_output_type_from_source(source))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns the response body as an `&str` for the given fetched response.
///
/// # Arguments
///
/// * `response` - The fetched response to extract the body from.
///
/// # Returns
///
/// `Some(&str)` if the response is HTML, `None` otherwise.
fn response_body_str(response: &FetchedResponse) -> Option<&str> {
    match response {
        FetchedResponse::Html(html) => Some(html.as_str()),
        _ => None,
    }
}

/// Walks a `>`-split path and returns a mutable reference to the target node,
/// creating intermediate nodes on demand.
///
/// # Arguments
///
/// * `item` - The root item node.
/// * `path` - Ordered path segments identifying the target node.
fn walk_mut<'a>(item: &'a mut ScraperDataNode, path: &[&str]) -> &'a mut ScraperDataNode {
    let mut current = item;
    for segment in path {
        current = current.children.entry((*segment).to_string()).or_default();
    }
    current
}

/// Walks an existing `>`-split path without creating output nodes.
fn get_node_mut<'a>(
    item: &'a mut ScraperDataNode,
    path: &[&str],
) -> Option<&'a mut ScraperDataNode> {
    let mut current = item;
    for segment in path {
        current = current.children.get_mut(*segment)?;
    }
    Some(current)
}

/// Returns `true` if the given string is a valid fetchable URL.
///
/// A valid fetch URL must start with `http://` or `https://` and have
/// a non-empty host component. This prevents sending requests to invalid
/// URLs with empty query parameters or unresolved placeholders.
fn is_valid_fetch_url(url: &str) -> bool {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return false;
    }
    // Check that the URL has no unresolved placeholders in the host portion.
    // The host is between "://" and the next "/" or end of string.
    if let Some(proto_end) = url.find("://") {
        let after_proto = url.get(proto_end + 3..).unwrap_or("");
        let host = if let Some(path_start) = after_proto.find('/') {
            after_proto.get(..path_start).unwrap_or("")
        } else {
            after_proto
        };
        if host.is_empty() || host.contains("{}") {
            return false;
        }
    }
    // Check that query parameter values are not empty (e.g. ?assetId=)
    if let Some(query_start) = url.find('?') {
        let query = url.get(query_start + 1..).unwrap_or("");
        for param in query.split('&') {
            if let Some(eq_pos) = param.find('=') {
                let value = param.get(eq_pos + 1..).unwrap_or("");
                if value.is_empty() {
                    return false;
                }
            }
        }
    }
    true
}

/// Deduplicates a list of URLs preserving order.
///
/// # Arguments
///
/// * `urls` - The list of URLs to deduplicate.
///
/// # Returns
///
/// A new vector with duplicate URLs removed, preserving the original order.
fn _dedupe_urls(urls: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for url in urls {
        if seen.insert(url.clone()) {
            out.push(url);
        }
    }
    out
}
