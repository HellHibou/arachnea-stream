//! Post-processing and targeted result-tree merging.

use super::*;

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
pub(super) async fn apply_post_processes(
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
pub(super) fn merge_targeted(
    item: &mut ScraperDataNode,
    source: ScraperDataNode,
    target: Option<&str>,
) {
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
///
/// # Arguments
///
/// * `item` - Current result item receiving merged fields or sub-query results.
/// * `sources` - Declared source identities and their default activation states.
/// * `target` - `>`-delimited destination path that receives the transformed result.
pub(super) fn merge_targeted_items(
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

/// Uses a scalar or array-element type from the source, falling back to string.
///
/// # Arguments
///
/// * `source` - Source result node whose type and values determine the merged output.
pub(super) fn scalar_output_type_from_source(source: &ScraperDataNode) -> ScraperOutputType {
    source
        .output_type
        .and_then(ScraperOutputType::array_element_type)
        .or(source.output_type)
        .filter(|output_type| output_type.is_scalar())
        .unwrap_or(ScraperOutputType::String)
}

/// Builds one scalar node using the source scalar type or array-element type.
///
/// # Arguments
///
/// * `value` - Input value interpreted by this operation.
/// * `source` - Source result node whose type and values determine the merged output.
pub(super) fn scalar_node_from_source_value(
    value: String,
    source: &ScraperDataNode,
) -> ScraperDataNode {
    ScraperDataNode::from_values_typed(vec![value], scalar_output_type_from_source(source))
}

/// Rebuilds one field from a merged sub-query result at the specified row index.
///
/// Scalar fields are converted to their element type, while object fields recurse
/// through their children so descriptors such as `resolver > target_id` remain
/// attached to the same array item as their sibling scalar fields.
///
/// # Arguments
///
/// * `source` - Source result node whose type and values determine the merged output.
/// * `index` - Zero-based row position; callers must keep it below `cardinality`.
/// * `cardinality` - Expected number of merged rows used to align flattened source values.
///
/// # Panics
///
/// Panics when a leaf has exactly `cardinality` values but `index` is outside that range.
pub(super) fn node_at_index(
    source: &ScraperDataNode,
    index: usize,
    cardinality: usize,
) -> Option<ScraperDataNode> {
    if source.children.is_empty() {
        return (source.values.len() == cardinality)
            .then(|| scalar_node_from_source_value(source.values[index].clone(), source));
    }

    let mut node = ScraperDataNode {
        output_type: source.output_type,
        values: Vec::new(),
        children: HashMap::new(),
        items: Vec::new(),
    };

    for (name, child) in &source.children {
        if let Some(child_node) = node_at_index(child, index, cardinality) {
            node.children.insert(name.clone(), child_node);
        }
    }

    (!node.children.is_empty()).then_some(node)
}

/// Walks a `>`-split path and returns a mutable reference to the target node,
/// creating intermediate nodes on demand.
///
/// # Arguments
///
/// * `item` - The root item node.
/// * `path` - Ordered path segments identifying the target node.
pub(super) fn walk_mut<'a>(
    item: &'a mut ScraperDataNode,
    path: &[&str],
) -> &'a mut ScraperDataNode {
    let mut current = item;
    for segment in path {
        current = current.children.entry((*segment).to_string()).or_default();
    }
    current
}

/// Walks an existing `>`-split path without creating output nodes.
///
/// # Arguments
///
/// * `item` - Current result item receiving merged fields or sub-query results.
/// * `path` - Path identifying the node in diagnostics or traversal.
pub(super) fn get_node_mut<'a>(
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
///
/// # Arguments
///
/// * `url` - Absolute request URL used by the HTTP transport.
pub(super) fn is_valid_fetch_url(url: &str) -> bool {
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
pub(super) fn _dedupe_urls(urls: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for url in urls {
        if seen.insert(url.clone()) {
            out.push(url);
        }
    }
    out
}
