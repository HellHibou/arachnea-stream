use anyhow::Result;
use std::collections::HashMap;
use std::sync::Mutex;

use crate::scrapyfy::query_helpers::{replace_template_placeholders, DynamicTemplateVariables};

/// Applies the `replace_variables` scraper action.
///
/// Only dynamic variables matching `variable_prefix` are made available to the
/// template renderer. Unknown placeholders and standard placeholders therefore
/// remain unchanged in the current values.
pub(super) fn apply(
    texts: Vec<String>,
    dynamic_variables: &Mutex<DynamicTemplateVariables>,
) -> Vec<String> {
    let Ok(dynamic_variables) = dynamic_variables.lock() else {
        return texts;
    };

    let params = dynamic_variables
        .as_map()
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<HashMap<_, _>>();

    texts
        .into_iter()
        .map(|value| replace_template_placeholders(&value, &params).0)
        .collect()
}

/// Validates the configuration of a `replace_variables` action.
pub(super) fn validate(_owner_name: &str, _owner: &str) -> Result<()> {
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/scrapyfy/actions/replace_variables.rs"]
mod tests;
