use super::super::scraper_data_node::{ScraperDataNode, ScraperOutputType};

/// Materializes aligned scalar fields as explicit object items, traversing parent arrays.
/// Singleton fields are broadcast; non-singleton fields keep their indexed value.
///
/// # Arguments
/// * `root` - Result subtree to update.
/// * `path` - Remaining child path, traversed through explicit parent items as well.
/// * `values_field` - Child whose value count defines the output length.
pub(super) fn apply(root: &mut ScraperDataNode, path: &[&str], values_field: &str) {
    if let Some((head, tail)) = path.split_first() {
        if *head == "**" {
            for child in root.children.values_mut() {
                apply(child, path, values_field);
            }
            for item in &mut root.items {
                apply(item, path, values_field);
            }
            apply(root, tail, values_field);
            return;
        }
        if let Some(child) = root.children.get_mut(*head) {
            apply(child, tail, values_field);
        }
        for item in &mut root.items {
            apply(item, path, values_field);
        }
        return;
    }
    let mut items = Vec::new();
    if root.items.is_empty() {
        append_items(root, values_field, &mut items);
    } else {
        for item in &root.items {
            append_items(item, values_field, &mut items);
        }
    }
    *root = ScraperDataNode {
        output_type: Some(ScraperOutputType::ObjectArray),
        items,
        ..Default::default()
    };
}

/// Appends one object for each primary scalar, skipping empty primary values.
///
/// # Arguments
/// * `node` - Container with aligned fields.
/// * `key` - Primary scalar child defining output cardinality.
/// * `items` - Destination receiving materialized objects.
fn append_items(node: &ScraperDataNode, key: &str, items: &mut Vec<ScraperDataNode>) {
    let Some(values) = node.children.get(key) else {
        return;
    };
    for (index, value) in values.values.iter().enumerate() {
        if value.trim().is_empty() {
            continue;
        }
        items.push(ScraperDataNode {
            children: node
                .children
                .iter()
                .map(|(name, child)| (name.clone(), project(child, index)))
                .collect(),
            ..Default::default()
        });
    }
}

/// Projects aligned values recursively, broadcasting only singleton fields.
/// Preserves previously materialized nested arrays.
///
/// # Arguments
/// * `node` - Field or nested container to project.
/// * `index` - Zero-based source index.
fn project(node: &ScraperDataNode, index: usize) -> ScraperDataNode {
    if !node.items.is_empty() {
        return node.clone();
    }
    ScraperDataNode {
        output_type: node
            .output_type
            .map(|kind| kind.array_element_type().unwrap_or(kind)),
        values: node
            .values
            .get(if node.values.len() == 1 { 0 } else { index })
            .cloned()
            .into_iter()
            .collect(),
        children: node
            .children
            .iter()
            .map(|(name, child)| (name.clone(), project(child, index)))
            .collect(),
        items: node.items.clone(),
    }
}
