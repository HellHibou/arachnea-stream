//! [`execute_query`] — unified polymorphic executor for every query type.
//!
//! Step 5 introduced the shared types ([`ScraperQuery`], [`ScraperEntrySpec`],
//! [`SubQuerySpec`], [`RowLocator`]). Step 15 implemented the dispatch logic
//! skeleton. Step 16 (this module) completes the wiring: proper downcasts via
//! [`ScraperEntrySpec::as_any`], request URL/header/body resolution, post-process
//! application, field filtering, and the public [`execute_query_items`] entry point
//! used by [`ScraperQueryDefinition`](crate::scrapyfy::scraper_query_collection::ScraperQueryDefinition).

use anyhow::Result;
use futures::stream::{self, StreamExt};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

use crate::scrapyfy::post_processes::{ScraperPostProcess, ScraperPostProcessContext};
use crate::scrapyfy::scraper_data_node::ScraperDataNode;
use crate::scrapyfy::scraper_html::entry::{HtmlScraperEntry, HtmlScraperSelectMode};
use crate::scrapyfy::scraper_json::entry::{select_json_values, JsonScraperEntry};
use crate::scrapyfy::scraper_static::query::StaticScraperEntryRaw;
use crate::scrapyfy::query_helpers;
use crate::scrapyfy::HttpClient;

use super::entry_trait::ScraperEntrySpec;
use super::query_trait::ScraperQuery;
use super::row_locator::RowLocator;
use super::sub_query_spec::SubQuerySpec;
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
    let items = execute_query_internal(query, context).await?;

    // Merge all items into a single root so that per-row fields
    // (e.g. episodes) accumulate into one array, while first-occurrence
    // scalar fields (marked with select: first) are kept once.
    let mut root = ScraperDataNode::default();
    for item in items {
        root.merge(item);
    }
    root.keep_first_values();

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
    // 1. Resolve request URL(s).
    let request_urls = resolve_request_urls(query, context);
    if request_urls.is_empty() {
        return Ok(Vec::new());
    }

    // 2. Fetch all responses in parallel.
    let responses = fetch_responses(query, &request_urls, context).await?;

    // 3. Build the merged root from items.
    let mut all_items = Vec::new();
    for (request_url, response) in responses {
        // a. Extract items from the response.
        let items = extract_items(query, &response, &request_url, context)?;

            for mut item in items {
                // b. Execute entry-level sub-queries (recursion).
                execute_entry_sub_queries(query, &mut item, &request_url, context).await?;

                // c. Execute sibling sub-queries (recursion via Box::pin).
                for sibling in query.sub_queries() {
                    // All sub-queries (query-level and entry-level) now use the
                    // same unified execution path. Query-level sub-queries that
                    // need context_pointer / request_pointer iteration have those
                    // fields set directly on the concrete query type (JsonScraperQuery /
                    // HtmlScraperQuery), and the unified executor handles them.
                    let sibling_root =
                        Box::pin(execute_query_internal(sibling, context)).await?;
                    let mut merged = ScraperDataNode::default();
                    for child in sibling_root {
                        merged.merge(child);
                    }
                    merge_targeted(&mut item, merged, sibling.sub_query_spec());
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

// ---------------------------------------------------------------------------
// Request resolution
// ---------------------------------------------------------------------------

/// Resolves the request URL by formatting the template with resolved params.
///
/// The URL template is resolved using [`query_helpers::format_query_template`]
/// with the parameters from the context. The `request_actions` pipeline is
/// applied at the body/header level, not the URL level.
///
/// # Arguments
///
/// * `query` - The query containing the URL template.
/// * `context` - Runtime context containing parameters for template resolution.
///
/// # Returns
///
/// A vector containing the resolved URL. Currently always returns a single URL.
fn resolve_request_urls(query: &dyn ScraperQuery, context: &QueryContext<'_>) -> Vec<String> {
    let formatted = query_helpers::format_query_template(
        query.base_url(),
        query.query_url(),
        context.params,
    )
    .unwrap_or_else(|_| query.query_url().to_string());

    vec![formatted]
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
///
/// # Returns
///
/// A hash map of header names to their resolved values.
fn resolve_request_headers(
    headers: &[crate::scrapyfy::scraper_json::query::ScraperRequestHeader],
    params: &HashMap<String, String>,
    request_url: &str,
) -> HashMap<String, String> {
    let request_context = query_helpers::build_params_json_value(params);
    let mut resolved = HashMap::new();
    for header in headers {
        if let Some((name, value)) = header.resolve(&request_context, params, request_url) {
            resolved.insert(name, value);
        }
    }
    resolved
}

/// Resolves the optional request body from the query's body pointer/actions.
///
/// Selects JSON values from the params context using the configured pointer,
/// then applies the body actions pipeline.
///
/// # Arguments
///
/// * `pointer` - Optional JSON pointer to select the body value.
/// * `select` - Selection mode for the pointer (First or All).
/// * `actions` - Actions to apply to the selected values.
/// * `params` - Runtime template parameters.
/// * `request_url` - URL of the parent request for action resolution.
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
) -> Option<String> {
    if pointer.is_none() && actions.is_empty() {
        return None;
    }
    let request_context = query_helpers::build_params_json_value(params);
    let mut values: Vec<String> = select_json_values(&request_context, pointer, select)
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
    );
    let body = resolve_request_body(
        query.request_pointer(),
        query.request_select(),
        query.request_actions(),
        context.params,
        urls.first().map(String::as_str).unwrap_or(""),
    );
    let extract_next_data = query.extract_next_data();
    let scraper_type = query.scraper_type();
    // Configure the client with the query's HTTP settings once, then clone
    // for each parallel job.
    let configured_client = context.http_client.configured(query.http_config().clone());

    let jobs = urls.iter().cloned().enumerate().map(|(index, url)| {
        let headers = headers.clone();
        let body = body.clone();
        let method = method.clone();
        let client = configured_client.clone();
        async move {
            let response = fetch_single(
                &client,
                method,
                &url,
                &headers,
                body.as_deref(),
                extract_next_data,
                scraper_type,
            )
            .await?;
            Ok::<(usize, (String, FetchedResponse)), anyhow::Error>((index, (url, response)))
        }
    });

    let results: Vec<Result<(usize, (String, FetchedResponse))>> =
        stream::iter(jobs).buffer_unordered(DEFAULT_FETCH_CONCURRENCY).collect().await;

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
                        html_entry.apply_to(&mut item, element, context.params, request_url, html_body);
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
            let values: Vec<&Value> = select_json_values(
                value,
                Some(&pointer),
                HtmlScraperSelectMode::All,
            );
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
    };
    Ok(result)
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
    // Collect entry names → values from the item so we can map sub-query
    // `request_pointer` to the entry value that seeds the URL.
    let entry_values: HashMap<String, Vec<String>> = collect_entry_values(item);

    for entry in query.entries() {
        // Determine which values from this entry seed sub-query URLs.
        let entry_name = entry.name().to_string();
        let parent_urls = entry_values.get(&entry_name).cloned().unwrap_or_default();
        let has_parent_urls = !parent_urls.is_empty();

        for sub_query in entry.sub_queries() {
            if has_parent_urls {
                // Entry-level sub-query: use the entry values as fetch URL(s).
                let rp = sub_query.request_pointer();
                if rp.is_none() || rp == Some(PARENT_SENTINEL) {
                    // Fetch each value as a URL, parse, extract, merge.
                    let mut merged = ScraperDataNode::default();
                    for url in &parent_urls {
                        let mut item_clone = ScraperDataNode::default();
                        let _ = fetch_and_extract_for_entry_sub_query(
                            sub_query, url, &mut item_clone, context,
                        ).await?;
                        merged.merge(item_clone);
                    }
                    // Clear the parent entry's values before merging the
                    // sub-query result — the sub-query *replaces* the URL
                    // with the decoded embed link(s).  Use the entry's
                    // name path for targeted merging.
                    let path: Vec<&str> = entry_name.split('>').map(str::trim).filter(|s| !s.is_empty()).collect();
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
                                                ScraperDataNode::from_values(
                                                    vec![values[i].clone()],
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
                                                ScraperDataNode::from_values(
                                                    vec![val.clone()],
                                                ),
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
                                                ScraperDataNode::from_values(
                                                    vec![val.clone()],
                                                ),
                                            );
                                            target.items.push(item_node);
                                        }
                                    }
                                    for val in &merged.values {
                                        let mut entry = ScraperDataNode::default();
                                        entry.values.push(val.clone());
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
                                            ScraperDataNode::from_values(
                                                vec![val.clone()],
                                            ),
                                        );
                                        target.items.push(item_node);
                                    }
                                }
                                for val in &merged.values {
                                    let mut entry = ScraperDataNode::default();
                                    entry.values.push(val.clone());
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
                                        ScraperDataNode::from_values(vec![val.clone()]),
                                    );
                                    target.items.push(item_node);
                                }
                            }
                            for val in &merged.values {
                                let mut entry = ScraperDataNode::default();
                                entry.values.push(val.clone());
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
                    };
                    let sub_items = Box::pin(execute_query_internal(sub_query, &sub_context)).await?;
                    let mut merged = ScraperDataNode::default();
                    for child in sub_items {
                        merged.merge(child);
                    }
                    merge_targeted(item, merged, sub_query.sub_query_spec());
                }
            } else {
                // No parent values available — try the generic executor path.
                let sub_context = QueryContext {
                    params: context.params,
                    request_url,
                    response_body: context.response_body,
                    http_client: context.http_client,
                    fields_filters: None,
                };
                let sub_items = Box::pin(execute_query_internal(sub_query, &sub_context)).await?;
                let mut merged = ScraperDataNode::default();
                for child in sub_items {
                    merged.merge(child);
                }
                merge_targeted(item, merged, sub_query.sub_query_spec());
            }
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
) -> Result<()> {
    let method = sub_query.request_method().as_http_method();
    let headers: HashMap<String, String> = resolve_request_headers(
        sub_query.request_headers(),
        context.params,
        url,
    );
    let body: Option<String> = resolve_request_body(
        sub_query.request_pointer(),
        sub_query.request_select(),
        sub_query.request_actions(),
        context.params,
        url,
    );
    let client = context.http_client.configured(sub_query.http_config().clone());
    let response = fetch_single(
        &client,
        method,
        url,
        &headers,
        body.as_deref(),
        sub_query.extract_next_data(),
        sub_query.scraper_type(),
    ).await?;

    match response {
        FetchedResponse::Html(html) => {
            let doc = ::scraper::Html::parse_document(&html);
            match sub_query.row_locator() {
                RowLocator::Selector { ref selector, select } => {
                    let sel = match ::scraper::Selector::parse(selector) {
                        Ok(s) => s,
                        Err(_) => return Ok(()),
                    };
                    let select_first = matches!(select, HtmlScraperSelectMode::First);
                    for element in doc.select(&sel) {
                        for entry in sub_query.entries() {
                            if let Some(html_entry) = as_html_entry(entry) {
                                html_entry.apply_to(item, element, context.params, url, Some(&html));
                            }
                        }
                        if select_first {
                            break;
                        }
                    }
                }
                RowLocator::Single
                | RowLocator::Pointer(_) => {
                    // No HTML element available for RowLocator::Single or
                    // RowLocator::Pointer — skip HTML entry application.
                }
                _ => {}
            }
        }
        FetchedResponse::Json(value) => {
            let pointer = match sub_query.row_locator() {
                RowLocator::Pointer(ref p) => p.clone(),
                _ => return Ok(()),
            };
            let rows = select_json_values(&value, Some(&pointer), HtmlScraperSelectMode::All);
            for row_value in rows {
                for entry in sub_query.entries() {
                    if let Some(json_entry) = as_json_entry(entry) {
                        json_entry.apply_to(item, row_value, context.params, url);
                    }
                }
            }
        }
        FetchedResponse::Static => {}
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
/// If a target path is specified in the sub-query spec, the source data is nested
/// under that path. Otherwise, the source is merged directly into the item.
///
/// # Arguments
///
/// * `item` - The item to merge the source into.
/// * `source` - The source data to merge.
/// * `spec` - Optional sub-query spec containing the target path.
fn merge_targeted(item: &mut ScraperDataNode, source: ScraperDataNode, spec: Option<&SubQuerySpec>) {
    if let Some(spec) = spec {
        if let Some(target) = &spec.target {
            let path: Vec<&str> = target.split('>').map(str::trim).filter(|s| !s.is_empty()).collect();
            if path.is_empty() {
                item.merge(source);
            } else {
                item.push_node(&path, source);
            }
            return;
        }
    }
    item.merge(source);
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

/// Clears the values and items at the node identified by a `>`-split path,
/// so the node is reset before merging sub-query results into it.
///
/// # Arguments
///
/// * `item` - The root item node.
/// * `path` - Ordered path segments identifying the target node.
fn clear_node_at_path(item: &mut ScraperDataNode, path: &[&str]) {
    let mut current = item;
    for segment in path {
        current = current.children.entry((*segment).to_string()).or_default();
    }
    current.values.clear();
    current.items.clear();
    // Also clear child nodes recursively so the node is fully clean.
    current.children.clear();
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

/// Deduplicates a list of URLs preserving order.
///
/// # Arguments
///
/// * `urls` - The list of URLs to deduplicate.
///
/// # Returns
///
/// A new vector with duplicate URLs removed, preserving the original order.
#[allow(dead_code)]
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

