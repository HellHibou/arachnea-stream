//! [`crate::scrapyfy::scraper::query_executor::execute_query_items`] — unified polymorphic executor for every query type.
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
use std::sync::Mutex;

use crate::scrapyfy::post_processes::{ScraperPostProcess, ScraperPostProcessContext};
use crate::scrapyfy::query_helpers::{self, DynamicTemplateVariables};
use crate::scrapyfy::scraper_data_node::{ScraperDataNode, ScraperOutputType};
use crate::scrapyfy::scraper_html::entry::{HtmlScraperEntry, HtmlScraperSelectMode};
use crate::scrapyfy::scraper_json::entry::{
    json_value_to_strings, select_json_values, JsonScraperEntry,
};
use crate::scrapyfy::scraper_json::query::{JsonScraperQuery, JsonScraperSubQuery};
use crate::scrapyfy::scraper_static::query::StaticScraperEntryRaw;
use crate::scrapyfy::{
    apply_pre_processes, fragment_from_response, hash62, ConditionalRequest, HttpClient,
    PreProcessAction, RootFetchOutcome, ScraperBrowserContext, ScraperBrowserTokenRetry,
    ScraperHttpExecution, ValidationSlot,
};

use super::entry_trait::ScraperEntrySpec;
use super::query_trait::ScraperQuery;
use super::row_locator::RowLocator;
use super::ScraperType;

use crate::QUERY_PARENT_SENTINEL as PARENT_SENTINEL;

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

    /// Request-scoped variables produced during extraction.
    ///
    /// The same storage is shared with sub-query contexts so action pipelines can
    /// add dynamic `@...` values once and later resolve them through the template
    /// placeholder mechanism.
    pub dynamic_template_variables: &'a Mutex<DynamicTemplateVariables>,

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

    /// Optional conditional-validation slot for the root request of a source.
    ///
    /// Only set on root contexts; every sub-query context clears it so
    /// conditional headers apply to the source's first request only.
    pub validation: Option<&'a ValidationSlot>,
}

use crate::QUERY_DEFAULT_FETCH_CONCURRENCY as DEFAULT_FETCH_CONCURRENCY;

use crate::QUERY_DEFAULT_ISOLATED_PAGE_CLICK_CONCURRENCY as DEFAULT_ISOLATED_PAGE_CLICK_CONCURRENCY;

/// Returns whether entries contain an isolated page-click sub-query at any depth.
///
/// # Arguments
///
/// * `entries` - Entry definitions applied to the current extraction context.
fn entries_contain_isolated_page_click(entries: &[&dyn ScraperEntrySpec]) -> bool {
    entries.iter().any(|entry| {
        entry.sub_queries().iter().any(|sub_query| {
            matches!(
                sub_query.http_config().execution,
                Some(ScraperHttpExecution::PageClick)
            ) && sub_query.http_config().browser_context == Some(ScraperBrowserContext::Isolated)
        }) || entries_contain_isolated_page_click(&entry.sub_entries())
    })
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

    /// Remote content unchanged (`304` or identical hash) — nothing to parse.
    ///
    /// Carries no body; the up-to-date validator fragment was recorded in the
    /// [`ValidationSlot`] by the conditional fetch path.
    NotModified,
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
    if let Some(url) = resolve_request_urls(query, context)?.into_iter().next() {
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
    let request_urls = resolve_request_urls(query, context)?;

    // 2a. Check for input_html mode: when the query provides an input_html
    //     template that resolves to a non-empty string, use it as the response
    //     body instead of fetching.
    let input_html_resolved = match query.input_html() {
        Some(template) if !template.is_empty() => {
            query_helpers::format_query_template(query.base_url(), template, context.params)
                .map_err(|error| {
                    anyhow::anyhow!(
                        "Failed to resolve input_html for query `{}`: {error}",
                        query.name()
                    )
                })
                .map(Some)?
                .filter(|value| !value.is_empty())
        }
        _ => None,
    };
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
                                )?;
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
                                        dynamic_template_variables: context
                                            .dynamic_template_variables,
                                        request_url: &request_url,
                                        response_body: context.response_body,
                                        http_client: context.http_client,
                                        fields_filters: None,
                                        parent_response: Some(ctx),
                                        validation: None,
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
                                                &sibling.http_config().empty_on_statuses,
                                                sibling.pre_processes(),
                                                sibling.name(),
                                                sibling.base_url(),
                                                context.params,
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
                                                        dynamic_template_variables: context
                                                            .dynamic_template_variables,
                                                        request_url: &url,
                                                        response_body: None,
                                                        http_client: context.http_client,
                                                        fields_filters: None,
                                                        parent_response: fetched_json,
                                                        validation: None,
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
                                                            dynamic_template_variables: context
                                                                .dynamic_template_variables,
                                                            request_url: &url,
                                                            response_body: None,
                                                            http_client: context.http_client,
                                                            fields_filters: None,
                                                            parent_response: row_value
                                                                .or(fetched_json),
                                                            validation: None,
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
                    dynamic_template_variables: context.dynamic_template_variables,
                    request_url: &request_url,
                    response_body: context.response_body,
                    http_client: context.http_client,
                    fields_filters: None,
                    parent_response: parent_json,
                    validation: None,
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

// ---------------------------------------------------------------------------
// Request resolution
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Fetch
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Row extraction
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Entry sub-queries
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Post-process
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Merge
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

mod fetch;
use fetch::*;
mod extraction;
use extraction::*;
mod sub_queries;
use sub_queries::*;
mod merge;
use merge::*;
