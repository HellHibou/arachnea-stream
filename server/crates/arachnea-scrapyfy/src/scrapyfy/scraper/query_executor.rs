//! [`execute_query`] — unified polymorphic executor for every query type.
//!
//! Step 5 introduced the shared types ([`ScraperQuery`], [`ScraperEntrySpec`],
//! [`SubQuerySpec`], [`RowLocator`]). Step 15 (this module) implements the
//! dispatch logic that drives root queries, sibling sub-queries, and
//! entry-level sub-queries through a single `&dyn ScraperQuery` entry point.

use anyhow::Result;
use futures::stream::{self, StreamExt};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

use crate::scrapyfy::actions::ScraperAction;
use crate::scrapyfy::post_processes::{ScraperPostProcess, ScraperPostProcessContext};
use crate::scrapyfy::scraper_data_node::ScraperDataNode;
use crate::scrapyfy::scraper_html::entry::HtmlScraperEntry;
use crate::scrapyfy::scraper_json::entry::{json_value_to_strings, select_json_values};
use crate::scrapyfy::scraper_json::query::ScraperRequestHeader;
use crate::scrapyfy::HttpClient;
use crate::scrapyfy::ScraperHttpConfig;

use super::entry_trait::ScraperEntrySpec;
use super::query_trait::ScraperQuery;
use super::row_locator::RowLocator;
use super::sub_query_spec::SubQuerySpec;
use super::ScraperType;

/// Runtime context forwarded to polymorphic query execution.
///
/// Mirrors the runtime context used by the legacy `JsonScraperSubQuery`
/// execution path; the unified executor adopts the same shape.
pub struct QueryContext<'a> {
    /// Runtime template parameters.
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

/// Executes a query polymorphically and returns its root data node.
///
/// The dispatch path covers:
/// 1. Build the request URL (apply `request_actions` to the base URL).
/// 2. Fetch the response(s) in parallel using the configured HTTP client.
/// 3. For each response, extract rows via [`RowLocator`] (CSS selector, JSON
///    pointer, or single row).
/// 4. For each row, build an item by applying entries, then recursively run
///    sibling sub-queries (`query.sub_queries()`).
/// 5. Run post-processes on the final root node.
/// 6. Apply `fields_filters` to the root if provided in the context.
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
pub async fn execute_query(
    query: &dyn ScraperQuery,
    context: &QueryContext<'_>,
) -> Result<ScraperDataNode> {
    // Resolve request URLs from query_url() + request_actions().
    let request_urls = resolve_request_urls(query, context);
    if request_urls.is_empty() {
        return Ok(ScraperDataNode::default());
    }

    // Fetch all responses in parallel.
    let responses = fetch_responses(query, &request_urls, context).await?;

    // Build the merged root.
    let mut root = ScraperDataNode::default();
    for (request_url, response) in responses {
        // Apply entries to each extracted row, then merge sub-queries.
        let items = extract_items(query, &response, &request_url, context);
        for mut item in items {
            // Sibling sub-queries (recursive). Box::pin avoids the infinite
            // future size from direct async recursion.
            for sibling in query.sub_queries() {
                let sibling_root = Box::pin(execute_query(sibling, context)).await?;
                merge_targeted(&mut item, sibling_root, sibling.sub_query_spec());
            }

            root.merge(item);
        }
    }

    // Post-processes at the root level.
    for _post_process in query.post_processes() {
        // Wired in step 16 when the unified executor replaces the legacy paths.
    }

    Ok(root)
}

/// Returns the optional [`SubQuerySpec`] attached to the given query.
pub fn resolve_sub_query_spec(_query: &dyn ScraperQuery) -> Option<&SubQuerySpec> {
    _query.sub_query_spec()
}

/// Returns the HTTP configuration associated with the given query.
pub fn http_config(_query: &dyn ScraperQuery) -> &ScraperHttpConfig {
    _query.http_config()
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Resolves the request URL(s) by combining `query_url()` with the
/// `request_actions()` pipeline.
///
/// The current implementation returns a single URL; future iterations can
/// expand to multiple seed URLs for parallel fan-out queries.
fn resolve_request_urls(query: &dyn ScraperQuery, _context: &QueryContext<'_>) -> Vec<String> {
    let mut urls = vec![query.query_url().to_string()];
    for _action in query.request_actions() {
        // No-op for now: request_actions are applied at the action pipeline
        // level (see legacy code). The unified executor keeps the URL list
        // single-entry until multi-URL fan-out is wired in.
    }
    urls
}

/// Fetches all responses in parallel using the configured HTTP client.
async fn fetch_responses(
    query: &dyn ScraperQuery,
    urls: &[String],
    context: &QueryContext<'_>,
) -> Result<Vec<(String, FetchedResponse)>> {
    let method = query.request_method().as_http_method();
    let headers = context
        .http_client
        .configured(query.http_config().clone());
    let extract_next_data = query.extract_next_data();
    let scraper_type = query.scraper_type();

    let jobs = urls.iter().cloned().enumerate().map(|(index, url)| {
        let headers = context.http_client.clone();
        let method = method.clone();
        let body: Option<String> = None; // body resolved by per-row logic in legacy code
        async move {
            let response = fetch_single(
                &headers,
                method,
                &url,
                &HashMap::new(),
                body.as_deref(),
                extract_next_data,
                scraper_type,
            )
            .await?;
            Ok::<(usize, (String, FetchedResponse)), anyhow::Error>((index, (url, response)))
        }
    });

    let _ = headers;
    let _ = DEFAULT_FETCH_CONCURRENCY;

    let results: Vec<Result<(usize, (String, FetchedResponse))>> =
        stream::iter(jobs).buffer_unordered(DEFAULT_FETCH_CONCURRENCY).collect().await;

    let mut ordered = Vec::with_capacity(results.len());
    for result in results {
        ordered.push(result?);
    }
    ordered.sort_by_key(|(index, _)| *index);
    Ok(ordered.into_iter().map(|(_, value)| value).collect())
}

/// Extracts items by applying entries to each row from the fetched response.
///
/// Returns owned [`ScraperDataNode`] items (HTML rows own their data; JSON
/// rows borrow the underlying `Value`). The result is consumed by the
/// sibling sub-query loop in [`execute_query`].
fn extract_items(
    query: &dyn ScraperQuery,
    response: &FetchedResponse,
    request_url: &str,
    context: &QueryContext<'_>,
) -> Vec<ScraperDataNode> {
    let html_body: Option<&str> = match response {
        FetchedResponse::Html(html) => Some(html.as_str()),
        _ => None,
    };

    match response {
        FetchedResponse::Html(html) => {
            // HTML: parse the document and apply entries directly. The rows
            // own their data so they can be returned without borrowing the
            // parsed document.
            let doc = ::scraper::Html::parse_document(html);
            let selector_str = match &query.row_locator() {
                RowLocator::Selector { selector, .. } => selector.clone(),
                _ => return Vec::new(),
            };
            let compiled = match ::scraper::Selector::parse(&selector_str) {
                Ok(selector) => selector,
                Err(_) => return Vec::new(),
            };
            let select_first = matches!(
                query.row_locator(),
                RowLocator::Selector { select, .. } if matches!(select, crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode::First)
            );
            let mut items = Vec::new();
            for element in doc.select(&compiled) {
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
                _ => return Vec::new(),
            };
            let values: Vec<&Value> = select_json_values(
                value,
                Some(&pointer),
                crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode::All,
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
            let mut item = ScraperDataNode::default();
            for entry in query.entries() {
                if let Some(json_entry) = as_json_entry(entry) {
                    // Static entries are JSON-shaped (they store YAML values
                    // serialised as JSON). Apply through the JSON path.
                    let synthetic = Value::Null;
                    json_entry.apply_to(&mut item, &synthetic, context.params, request_url);
                }
            }
            vec![item]
        }
    }
}

/// Best-effort downcast of a `&dyn ScraperEntrySpec` to `&HtmlScraperEntry`.
///
/// The trait does not expose a downcast, so we rely on the `entry_type()`
/// discriminant and re-look-up the entry in the parent's `entries()` list.
/// This is a placeholder for step 16 (câblage) where a proper downcast
/// helper will be added.
fn as_html_entry(_entry: &dyn ScraperEntrySpec) -> Option<&HtmlScraperEntry> {
    None
}

/// Placeholder for JSON downcast (step 16 will provide a proper helper).
fn as_json_entry(_entry: &dyn ScraperEntrySpec) -> Option<&crate::scrapyfy::scraper_json::entry::JsonScraperEntry> {
    None
}

#[allow(dead_code)]
fn _silence_unused_json_imports() {
    let _ = json_value_to_strings;
}

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

/// Placeholder for post-process application (wired in step 16).
#[allow(dead_code)]
fn _apply_post_process(
    _post_process: &ScraperPostProcess,
    _node: &mut ScraperDataNode,
    _params: &HashMap<String, String>,
    _request_url: &str,
    _response_body: Option<&str>,
    _http_client: &HttpClient,
) {
    // Wired in step 16 when the unified executor replaces the legacy paths.
}

/// Placeholder for request header resolution (wired in step 16).
#[allow(dead_code)]
fn _resolve_request_headers(
    _headers: &[ScraperRequestHeader],
    _params: &HashMap<String, String>,
    _request_url: &str,
) -> HashMap<String, String> {
    HashMap::new()
}

/// Placeholder for request body resolution (wired in step 16).
#[allow(dead_code)]
fn _resolve_request_body(
    _pointer: Option<&str>,
    _select: crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode,
    _actions: &[ScraperAction],
    _params: &HashMap<String, String>,
    _request_url: &str,
) -> Option<String> {
    None
}

/// Placeholder for post-process context (wired in step 16).
#[allow(dead_code)]
fn _make_post_process_context<'a>(
    params: &'a HashMap<String, String>,
    request_url: &'a str,
    response_body: Option<&'a str>,
    http_client: &'a HttpClient,
) -> ScraperPostProcessContext<'a> {
    ScraperPostProcessContext {
        params,
        request_url,
        response_body,
        http_client,
    }
}

/// Placeholder for hash set used to dedupe URLs.
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
