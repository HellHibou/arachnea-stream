use anyhow::{anyhow, Result};

use crate::scrapyfy::{ScraperAction, ScraperDataNode, ScraperOutputType};

use super::{
    node_helpers::{get_node, set_node, split_path},
    ScraperPostProcessContext,
};

/// Validates a generic action pipeline applied to a root scalar field.
pub(super) fn validate(
    owner: &str,
    source: &str,
    target: &str,
    actions: &[ScraperAction],
) -> Result<()> {
    if source.trim().is_empty() || target.trim().is_empty() {
        return Err(anyhow!(
            "{} apply_actions_to_field requires non-empty source and target",
            owner
        ));
    }
    if actions.is_empty() {
        return Err(anyhow!(
            "{} apply_actions_to_field requires at least one action",
            owner
        ));
    }
    for action in actions {
        action.validate(target, "apply_actions_to_field")?;
    }
    Ok(())
}

/// Applies an action pipeline to values read from `source` and writes the
/// result to `target`, creating the target path when necessary.
pub(super) fn apply(
    root: &mut ScraperDataNode,
    context: &ScraperPostProcessContext<'_>,
    source: &str,
    target: &str,
    output_type: Option<ScraperOutputType>,
    actions: &[ScraperAction],
) {
    let Some(source_node) = get_node(root, source) else {
        return;
    };
    let mut values = source_node.values.clone();
    for action in actions {
        values = action.apply(
            &None,
            values,
            context.params,
            context.request_url,
            context.response_body,
            None,
        );
    }

    let mut target_node = ScraperDataNode::from_values(values);
    if let Some(output_type) = output_type {
        target_node.set_self_output_type(output_type);
    }
    set_node(root, &split_path(target), target_node);
}
