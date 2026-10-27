//! Entry sub-query orchestration and bounded follow-up fetching.

use super::*;

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
pub(super) async fn execute_entry_sub_queries(
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
pub(super) async fn execute_entry_sub_queries_for_entries(
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
                    let valid_urls: Vec<(usize, String)> = parent_urls
                        .iter()
                        .enumerate()
                        .filter_map(|(index, url)| {
                            let trimmed = url.trim();
                            (!trimmed.is_empty()
                                && !trimmed.contains("{}")
                                && is_valid_fetch_url(trimmed))
                            .then(|| (index, url.clone()))
                        })
                        .collect();
                    let isolated_page_click = matches!(
                        sub_query.http_config().execution,
                        Some(ScraperHttpExecution::PageClick)
                    ) && sub_query.http_config().browser_context
                        == Some(ScraperBrowserContext::Isolated);
                    let mut merged = ScraperDataNode::default();
                    if isolated_page_click {
                        let jobs = valid_urls.into_iter().map(|(index, url)| {
                            let params = sub_query_params.clone();
                            async move {
                                let mut item_clone = ScraperDataNode::default();
                                fetch_and_extract_for_entry_sub_query(
                                    sub_query,
                                    &url,
                                    &mut item_clone,
                                    context,
                                    &params,
                                )
                                .await?;
                                Ok::<(usize, ScraperDataNode), anyhow::Error>((index, item_clone))
                            }
                        });
                        let mut results: Vec<(usize, ScraperDataNode)> = stream::iter(jobs)
                            .buffer_unordered(DEFAULT_ISOLATED_PAGE_CLICK_CONCURRENCY)
                            .collect::<Vec<Result<(usize, ScraperDataNode)>>>()
                            .await
                            .into_iter()
                            .collect::<Result<_>>()?;
                        results.sort_by_key(|(index, _)| *index);
                        for (_, item_clone) in results {
                            merged.merge(item_clone);
                        }
                    } else {
                        for (_, url) in valid_urls {
                            let mut item_clone = ScraperDataNode::default();
                            fetch_and_extract_for_entry_sub_query(
                                sub_query,
                                &url,
                                &mut item_clone,
                                context,
                                &sub_query_params,
                            )
                            .await?;
                            merged.merge(item_clone);
                        }
                    }
                    // `target: parent` keeps the source entry intact and merges
                    // extracted fields into its containing output object. This
                    // is useful when a link field triggers an enrichment fetch
                    // that should add sibling fields such as `web-link` or
                    // `players` rather than become children of `link`.
                    if sub_query.target() == Some("parent") {
                        item.merge(merged);
                        continue;
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
                                let mut common: Vec<(&String, &ScraperDataNode)> = Vec::new();
                                let mut leftover: Vec<(&String, &ScraperDataNode)> = Vec::new();
                                for (name, child) in &merged.children {
                                    if child.values.len() == cardinality
                                        || !child.children.is_empty()
                                    {
                                        common.push((name, child));
                                    } else if !child.values.is_empty() {
                                        leftover.push((name, child));
                                    }
                                }
                                if common.len() >= 2 {
                                    // Zip scalar fields and recursively rebuild nested objects.
                                    for i in 0..cardinality {
                                        let mut item_node = ScraperDataNode::default();
                                        for (name, child) in &common {
                                            if let Some(node) = node_at_index(child, i, cardinality)
                                            {
                                                item_node.children.insert((*name).clone(), node);
                                            }
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
                        dynamic_template_variables: context.dynamic_template_variables,
                        request_url,
                        response_body: context.response_body,
                        http_client: context.http_client,
                        fields_filters: None,
                        parent_response: None,
                        validation: None,
                    };
                    let sub_items =
                        Box::pin(execute_query_internal(sub_query, &sub_context)).await?;
                    merge_targeted_items(item, sub_items, sub_query.target());
                }
            } else {
                // No parent values available — try the generic executor path.
                let sub_context = QueryContext {
                    params: context.params,
                    dynamic_template_variables: context.dynamic_template_variables,
                    request_url,
                    response_body: context.response_body,
                    http_client: context.http_client,
                    fields_filters: None,
                    parent_response: None,
                    validation: None,
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
        if entries_contain_isolated_page_click(&child_entries) {
            let group_items = std::mem::take(&mut group.items);
            let jobs = group_items
                .into_iter()
                .enumerate()
                .map(|(index, mut group_item)| {
                    let child_entries = child_entries.clone();
                    async move {
                        Box::pin(execute_entry_sub_queries_for_entries(
                            &child_entries,
                            &mut group_item,
                            request_url,
                            context,
                        ))
                        .await?;
                        Ok::<(usize, ScraperDataNode), anyhow::Error>((index, group_item))
                    }
                });
            let mut results: Vec<(usize, ScraperDataNode)> = stream::iter(jobs)
                .buffer_unordered(DEFAULT_ISOLATED_PAGE_CLICK_CONCURRENCY)
                .collect::<Vec<Result<(usize, ScraperDataNode)>>>()
                .await
                .into_iter()
                .collect::<Result<_>>()?;
            results.sort_by_key(|(index, _)| *index);
            group.items = results
                .into_iter()
                .map(|(_, group_item)| group_item)
                .collect();
        } else {
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
pub(super) fn collect_entry_values(item: &ScraperDataNode) -> HashMap<String, Vec<String>> {
    let mut map = HashMap::new();
    collect_entry_values_recursive(item, &mut map, empty_path());
    map
}

/// Adds current entry values to the template context of an entry sub-query.
///
/// Both the original field name and an underscore-normalized variant are
/// available, so YAML can reference a field such as `player-id` through the
/// placeholder `{player_id}` accepted by the template grammar.
pub(super) fn build_sub_query_params(
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
pub(super) fn collect_entry_values_recursive<'a>(
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
pub(super) async fn fetch_and_extract_for_entry_sub_query(
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
                Ok(html) => {
                    let html = apply_pre_processes(
                        html,
                        sub_query.pre_processes(),
                        sub_query.name(),
                        sub_query.base_url(),
                        params,
                    )?;
                    break FetchedResponse::Html(html);
                }
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
        let browser_context = sub_query.http_config().browser_context;
        if !matches!(
            browser_context,
            Some(ScraperBrowserContext::Origin | ScraperBrowserContext::Isolated)
        ) {
            anyhow::bail!("page_click requires http.browser_context: origin or isolated");
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
        let html = if browser_context == Some(ScraperBrowserContext::Isolated) {
            client
                .page_click_isolated_for_request(&page_url, &selector, &wait_for_selector)
                .await?
        } else {
            client
                .page_click_for_request(&page_url, &selector, &wait_for_selector)
                .await?
        };
        FetchedResponse::Html(apply_pre_processes(
            html,
            sub_query.pre_processes(),
            sub_query.name(),
            sub_query.base_url(),
            params,
        )?)
    } else {
        fetch_single(
            &client,
            method,
            url,
            &headers,
            body.as_deref(),
            sub_query.extract_next_data(),
            sub_query.scraper_type(),
            &sub_query.http_config().empty_on_statuses,
            sub_query.pre_processes(),
            sub_query.name(),
            sub_query.base_url(),
            params,
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
                    let selector = resolve_html_selector(selector, context.params);
                    let sel = match ::scraper::Selector::parse(&selector) {
                        Ok(s) => s,
                        Err(_) => return Ok(()),
                    };
                    let select_first = matches!(select, HtmlScraperSelectMode::First);
                    for element in doc.select(&sel) {
                        for entry in sub_query.entries() {
                            if let Some(html_entry) = as_html_entry(entry) {
                                html_entry.apply_to(
                                    item,
                                    element,
                                    context.params,
                                    context.dynamic_template_variables,
                                    url,
                                    Some(&html),
                                )?;
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
                        json_entry.apply_to(
                            item,
                            row_value,
                            context.params,
                            context.dynamic_template_variables,
                            url,
                        )?;
                    }
                }
            }
            nested_parent_response = Some(value);
        }
        FetchedResponse::Text(_) => {}
        FetchedResponse::Static => {}
        FetchedResponse::NotModified => {}
    }

    if !sub_query.entries().is_empty() {
        let sub_context = QueryContext {
            params: context.params,
            dynamic_template_variables: context.dynamic_template_variables,
            request_url: url,
            response_body: nested_response_body.as_deref(),
            http_client: context.http_client,
            fields_filters: None,
            parent_response: nested_parent_response.as_ref(),
            validation: None,
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
