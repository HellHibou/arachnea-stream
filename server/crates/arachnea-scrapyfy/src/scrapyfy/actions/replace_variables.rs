use anyhow::Result;
use std::collections::HashMap;
use std::sync::Mutex;

use crate::scrapyfy::query_helpers::{
    replace_template_placeholders, DynamicTemplateVariables, DYNAMIC_TEMPLATE_VARIABLE_PREFIX,
};

/// Applies the `replace_variables` scraper action.
///
/// Only dynamic variables matching `variable_prefix` are made available to the
/// template renderer. Unknown placeholders and standard placeholders therefore
/// remain unchanged in the current values.
pub(super) fn apply(
    texts: Vec<String>,
    variable_prefix: &str,
    dynamic_variables: &Mutex<DynamicTemplateVariables>,
) -> Vec<String> {
    if variable_prefix != DYNAMIC_TEMPLATE_VARIABLE_PREFIX {
        return texts;
    }

    let Ok(dynamic_variables) = dynamic_variables.lock() else {
        return texts;
    };

    let params = dynamic_variables
        .as_map()
        .iter()
        .filter(|(key, _)| key.starts_with(variable_prefix))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<HashMap<_, _>>();

    texts
        .into_iter()
        .map(|value| replace_template_placeholders(&value, &params).0)
        .collect()
}

/// Validates the configuration of a `replace_variables` action.
pub(super) fn validate(owner_name: &str, owner: &str, variable_prefix: &str) -> Result<()> {
    if variable_prefix != DYNAMIC_TEMPLATE_VARIABLE_PREFIX {
        anyhow::bail!(
            "{} {} has unsupported replace_variables variable_prefix {}; only {} is supported",
            owner,
            owner_name,
            variable_prefix,
            DYNAMIC_TEMPLATE_VARIABLE_PREFIX
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_replaces_only_dynamic_placeholders() {
        let params = HashMap::new();
        let dynamic = Mutex::new(DynamicTemplateVariables::new());
        dynamic.lock().unwrap().insert(&params, "@A", "80").unwrap();

        let values = apply(
            vec!["{@A}:{country}:{@Missing}".to_string()],
            DYNAMIC_TEMPLATE_VARIABLE_PREFIX,
            &dynamic,
        );

        assert_eq!(values, vec!["80:{country}:{@Missing}".to_string()]);
    }

    #[test]
    fn validate_rejects_non_dynamic_prefix() {
        let error = validate("port", "entry", "$").unwrap_err();

        assert!(error.to_string().contains("unsupported replace_variables"));
    }
}
