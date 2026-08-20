use anyhow::{Context, Result};
use futures::stream::{self, StreamExt};
use regex::Regex;
use std::collections::HashSet;

use super::super::actions::ScraperAction;
use super::super::scraper_data_node::ScraperDataNode;
use super::node_helpers::{get_node, get_node_mut, prepare_copy_fields, set_node, split_path};
use super::regex_helpers::build_regex_items;
use super::types::{ScraperFieldMapping, ScraperPostProcessContext, ScraperRegexItemEntry};
use crate::scrapyfy::scraper::config::ScraperRequestMethod;
use crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode;
use crate::scrapyfy::scraper_json::entry::{json_value_to_strings, select_json_values};

/// Maximum number of in-flight HTTP requests issued by a single
/// `fetch_regex_items_from_items` post-process step.
const POST_PROCESS_FETCH_CONCURRENCY: usize = 8;

/// Validates one `fetch_regex_items_from_items` post-process definition.
///
/// # Arguments
///
/// * `owner` - Human-readable owner name used in error messages.
/// * `pattern` - Regular expression source to compile.
/// * `entries` - Field definitions whose `actions` are recursively validated.
/// * `request_actions` - Additional actions applied to the resolved request
///   URL; their validation is also performed.
///
/// # Errors
///
/// Returns an error when `pattern` does not compile, or when one of the
/// entry/request actions fails to validate.
pub(super) fn validate(
    owner: &str,
    pattern: &str,
    entries: &[ScraperRegexItemEntry],
    request_actions: &[ScraperAction],
) -> Result<()> {
    Regex::new(pattern).with_context(|| format!("Invalid post-process regex for {}", owner))?;
    for entry in entries {
        let Some(output_type) = entry.output_type else {
            anyhow::bail!(
                "fetch_regex_items_from_items for {} entry {} must define type",
                owner,
                entry.name
            );
        };
        if matches!(
            output_type,
            crate::scrapyfy::ScraperOutputType::Object
                | crate::scrapyfy::ScraperOutputType::ObjectArray
        ) {
            anyhow::bail!(
                "fetch_regex_items_from_items for {} entry {} cannot use {} without nested entries",
                owner,
                entry.name,
                output_type.as_str()
            );
        }
        for action in &entry.actions {
            action.validate(&entry.name, "post-process entry")?;
        }
    }
    for action in request_actions {
        action.validate(owner, "post-process request")?;
    }
    Ok(())
}

/// Fetches one text payload per resolved request URL, applies a regex on each
/// response body, and appends the produced items under `target`.
///
/// Request URLs are deduplicated, prepared with `request_actions`, and fetched
/// in parallel with at most [`POST_PROCESS_FETCH_CONCURRENCY`] in-flight calls.
/// `copy_item_fields` is applied to every produced item so that values lifted
/// from the source item travel alongside the regex matches.
///
/// # Arguments
///
/// * `root` - Root node that owns the source group and receives the items.
/// * `context` - Runtime context providing params, the request URL, and the
///   shared HTTP client.
/// * `source` - `>`-delimited path of the source group.
/// * `request_field` - `>`-delimited path of the field whose values are used
///   to build the request URLs.
/// * `request_actions` - Actions applied to the resolved request URLs.
/// * `target` - `>`-delimited path that receives the produced items.
/// * `pattern` - Regular expression applied to every response body.
/// * `entries` - Field definitions used to build each produced item.
/// * `copy_item_fields` - Field mappings cloned from the source item into
///   every produced item.
///
/// # Errors
///
/// Returns an error when one of the HTTP requests fails.
#[allow(clippy::too_many_arguments)]
pub(super) async fn apply(
    root: &mut ScraperDataNode,
    context: &ScraperPostProcessContext<'_>,
    source: &str,
    request_field: &str,
    request_actions: &[ScraperAction],
    request_method: ScraperRequestMethod,
    response_pointer: Option<&str>,
    target: &str,
    target_per_source: bool,
    pattern: &str,
    entries: &[ScraperRegexItemEntry],
    copy_item_fields: &[ScraperFieldMapping],
) -> Result<()> {
    struct FetchRegexItemsJob {
        source_index: usize,
        request_url: String,
        prepared_copy_fields: Vec<(String, ScraperDataNode)>,
    }

    let mut extracted_items = Vec::new();
    let mut seen_request_urls = HashSet::new();
    let mut fetch_jobs = Vec::new();
    let target_path = split_path(target);

    {
        let Some(source_node) = get_node(root, source) else {
            return Ok(());
        };

        if source_node.items.is_empty() {
            return Ok(());
        }

        for (source_index, source_item) in source_node.items.iter().enumerate() {
            let Some(request_values) =
                get_node(source_item, request_field).map(|node| node.values.clone())
            else {
                continue;
            };

            let mut resolved_request_urls = request_values;
            for action in request_actions {
                resolved_request_urls = action.apply(
                    &None,
                    resolved_request_urls,
                    context.params,
                    context.request_url,
                    context.response_body,
                    None,
                );
            }

            for request_url in resolved_request_urls
                .into_iter()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
            {
                if !seen_request_urls.insert(request_url.clone()) {
                    continue;
                }

                fetch_jobs.push(FetchRegexItemsJob {
                    source_index,
                    request_url,
                    prepared_copy_fields: prepare_copy_fields(source_item, copy_item_fields),
                });
            }
        }
    }

    let fetch_results = stream::iter(fetch_jobs.into_iter().map(|job| async move {
        let response_body = context
            .http_client
            .query_http(request_method.as_http_method(), &job.request_url)
            .await
            .with_context(|| format!("Failed to fetch post-process request {}", job.request_url))?;

        Ok::<(FetchRegexItemsJob, String), anyhow::Error>((job, response_body))
    }))
    .buffer_unordered(POST_PROCESS_FETCH_CONCURRENCY)
    .collect::<Vec<_>>()
    .await;

    for fetch_result in fetch_results {
        let (job, response_body) = fetch_result?;
        let response_texts = if let Some(pointer) = response_pointer {
            let response_json: serde_json::Value = serde_json::from_str(&response_body)
                .with_context(|| {
                    format!("Invalid JSON post-process response {}", job.request_url)
                })?;
            select_json_values(&response_json, Some(pointer), HtmlScraperSelectMode::All)
                .into_iter()
                .flat_map(json_value_to_strings)
                .collect()
        } else {
            vec![response_body.clone()]
        };

        for response_text in response_texts {
            for mut item in build_regex_items(
                &response_text,
                pattern,
                entries,
                context.params,
                &job.request_url,
                Some(&response_body),
            ) {
                for (target, node) in &job.prepared_copy_fields {
                    set_node(&mut item, &split_path(target), node.clone());
                }

                if target_per_source {
                    let Some(source_node) = get_node_mut(root, source) else {
                        continue;
                    };
                    let Some(source_item) = source_node.items.get_mut(job.source_index) else {
                        continue;
                    };
                    source_item.push_node_typed(
                        &target_path,
                        item,
                        crate::scrapyfy::ScraperOutputType::ObjectArray,
                    );
                } else {
                    extracted_items.push(item);
                }
            }
        }
    }

    if target_per_source {
        return Ok(());
    }

    root.set_output_type(
        &target_path,
        crate::scrapyfy::ScraperOutputType::ObjectArray,
    );
    for item in extracted_items {
        root.push_node_typed(
            &target_path,
            item,
            crate::scrapyfy::ScraperOutputType::ObjectArray,
        );
    }

    Ok(())
}
