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
use crate::scrapyfy::scraper_json::query::JsonScraperSubQuery;
use crate::scrapyfy::scraper_static::query::StaticScraperEntryRaw;
use crate::scrapyfy::query_helpers;
use crate::scrapyfy::HttpClient;

use super::entry_trait::ScraperEntrySpec;
use super::query_trait::ScraperQuery;
use super::row_locator::RowLocator;
use super::sub_query_spec::SubQuerySpec;
use super::ScraperType;

/// Runtime context forwarded to polymorphic query execution.
///
/// Mirrors the runtime context used by the legacy per-type `execute_query`
/// methods; the unified executor adopts the same shape.
pub struct QueryContext<'a> {
    /// Runtime template parameters (pre-resolved by the caller with collection
    /// defaults and `query_param_mappings` already applied).
    pub params: &'a HashMap<String, String>,
    /// URL of the parent request for action resolution.
    pub request_url: &'a str,
    /// Optional parent response body available to actions.
    pub response_body: Option<&'a str>,
    /// Shared HTTP client.
    pub http_client: &'a HttpClient,
    /// Optional root fields filter list forwarded to `fields_filters`.
    pub fields_filters: Option<&'a HashMap<String, Vec<String>>>,
}

/// Maximum number of follow-up requests executed together (sane default for
/// both HTML and JSON; legacy code used 8 for JSON, 4 for HTML siblings).
const DEFAULT_FETCH_CONCURRENCY: usize = 8;

/// Fetches a single URL using the configured HTTP method, headers, and body.
///
/// Dispatches on `extract_next_data` for JSON queries; HTML always returns
/// the response body as text.
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
pub enum FetchedResponse {
    /// Raw HTML body for HTML queries.
    Html(String),
    /// Parsed JSON value for JSON queries.
    Json(Value),
    /// No response (static queries).
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
                    // Any `JsonScraperSubQuery` / `HtmlScraperSubQuery`
                    // sibling uses the legacy query-level API (its
                    // `query_url` is empty, it walks the parent row via
                    // `context_pointer`/`row_pointer`, and merges under
                    // `target`). The unified executor therefore dispatches
                    // every concrete query-level sub-query through its
                    // legacy `execute_query_level` entry point, regardless
                    // of whether `context_pointer` is `Some` (one context
                    // per row of the parent) or `None` (single context =
                    // the whole parent row).
                    if let Some(json_sub) = sibling
                        .as_any()
                        .downcast_ref::<JsonScraperSubQuery>()
                    {
                        // The parent row is the *raw response* of the
                        // current query (not the item we just built): the
                        // legacy `JsonScraperSubQuery::execute` walks it via
                        // `context_pointer` to find the per-context values
                        // (e.g. `/episodes/*`). Passing the item would drop
                        // the `episodes` array.
                        let parent_row = response_parent_row(&response);
                        let execution_options = query_level_execution_options();
                        let sub_client = context
                            .http_client
                            .configured(sibling.http_config().clone());
                        let merged = json_sub
                            .execute_query_level(
                                &parent_row,
                                context.params,
                                &request_url,
                                query.base_url(),
                                &sub_client,
                                execution_options,
                            )
                            .await?;
                        merge_targeted(&mut item, merged, sibling.sub_query_spec());
                    } else {
                        // Entry-level sub-query (or HTML query-level — not
                        // yet supported): recurse through the unified
                        // executor.
                        let sibling_root =
                            Box::pin(execute_query_internal(sibling, context)).await?;
                        let mut merged = ScraperDataNode::default();
                        for child in sibling_root {
                            merged.merge(child);
                        }
                        merge_targeted(&mut item, merged, sibling.sub_query_spec());
                    }
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
fn as_html_entry(entry: &dyn ScraperEntrySpec) -> Option<&HtmlScraperEntry> {
    entry.as_any().downcast_ref::<HtmlScraperEntry>()
}

/// Downcasts a `&dyn ScraperEntrySpec` to `&JsonScraperEntry` via
/// [`std::any::Any`].
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
async fn execute_entry_sub_queries(
    query: &dyn ScraperQuery,
    item: &mut ScraperDataNode,
    request_url: &str,
    context: &QueryContext<'_>,
) -> Result<()> {
    for entry in query.entries() {
        for sub_query in entry.sub_queries() {
            // Build a context for the sub-query using the item's data.
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
    Ok(())
}

// ---------------------------------------------------------------------------
// Post-process
// ---------------------------------------------------------------------------

/// Applies the query's post-process pipeline to a single item.
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
fn response_body_str(response: &FetchedResponse) -> Option<&str> {
    match response {
        FetchedResponse::Html(html) => Some(html.as_str()),
        _ => None,
    }
}

/// Deduplicates a list of URLs preserving order.
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

/// Builds a `serde_json::Value` representation of the **raw response** that
/// the query-level sub-query iterates over via its `context_pointer`.
///
/// The legacy `JsonScraperSubQuery::execute` walks the parent row with
/// `select_json_values(context_row, context_pointer, context_select)`. For a
/// root-level sub-query, the parent row is the **root response** of the
/// parent query (e.g. the season API payload containing `episodes: [...]`).
/// For HTML and Static we wrap the body in a `Value::Null` so the legacy
/// sub-query short-circuits without attempting a JSON walk.
fn response_parent_row(response: &FetchedResponse) -> Value {
    match response {
        FetchedResponse::Json(value) => value.clone(),
        FetchedResponse::Html(_) | FetchedResponse::Static => Value::Null,
    }
}

/// Returns the default concurrency options for query-level sub-queries.
///
/// Mirrors the legacy `JsonScraperQuery` defaults: 4 sibling, 4 context, 8 fetch.
fn query_level_execution_options(
) -> crate::scrapyfy::scraper_json::config::JsonScraperExecutionOptions {
    crate::scrapyfy::scraper_json::config::JsonScraperExecutionOptions {
        sibling_sub_query_concurrency: 4,
        sub_query_context_concurrency: 4,
        sub_query_fetch_concurrency: DEFAULT_FETCH_CONCURRENCY,
    }
}
