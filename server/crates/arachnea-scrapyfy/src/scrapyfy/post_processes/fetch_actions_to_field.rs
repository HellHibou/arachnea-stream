use anyhow::{anyhow, Context, Result};
use http::Method;

use crate::scrapyfy::{ScraperAction, ScraperDataNode, ScraperOutputType};

use super::{
    node_helpers::{get_node, set_node, split_path},
    ScraperPostProcessContext,
};

/// Validates a scalar follow-up fetch and action pipeline.
pub(super) fn validate(
    owner: &str,
    source: &str,
    target: &str,
    actions: &[ScraperAction],
    request_actions: &[ScraperAction],
) -> Result<()> {
    if source.trim().is_empty() || target.trim().is_empty() {
        return Err(anyhow!(
            "{} fetch_actions_to_field requires non-empty source and target",
            owner
        ));
    }
    if actions.is_empty() {
        return Err(anyhow!(
            "{} fetch_actions_to_field requires at least one action",
            owner
        ));
    }
    for action in request_actions {
        action.validate(source, "fetch_actions_to_field request")?;
    }
    for action in actions {
        action.validate(target, "fetch_actions_to_field")?;
    }
    Ok(())
}

/// Fetches every URL stored in `source`, applies `actions` to each response,
/// and replaces `target` with the combined output values.
pub(super) async fn apply(
    root: &mut ScraperDataNode,
    context: &ScraperPostProcessContext<'_>,
    source: &str,
    target: &str,
    output_type: Option<ScraperOutputType>,
    request_actions: &[ScraperAction],
    actions: &[ScraperAction],
) -> Result<()> {
    let Some(source_node) = get_node(root, source) else {
        return Ok(());
    };

    let mut request_urls = source_node.values.clone();
    for action in request_actions {
        request_urls = action.apply(
            &None,
            request_urls,
            context.params,
            context.request_url,
            context.response_body,
            None,
        );
    }

    let mut output_values = Vec::new();
    for request_url in request_urls
        .into_iter()
        .filter(|value| !value.trim().is_empty())
    {
        let response_body = context
            .http_client
            .query_http(Method::GET, &request_url)
            .await
            .with_context(|| format!("Failed to fetch post-process request {}", request_url))?;
        let mut values = Vec::new();
        for action in actions {
            values = action.apply(
                &None,
                values,
                context.params,
                &request_url,
                Some(&response_body),
                None,
            );
        }
        output_values.extend(values);
    }

    if !output_values.is_empty() {
        let mut target_node = ScraperDataNode::from_values(output_values);
        if let Some(output_type) = output_type {
            target_node.set_self_output_type(output_type);
        }
        set_node(root, &split_path(target), target_node);
    }
    Ok(())
}
