use anyhow::{bail, Context, Result};

use super::super::query_helpers;

/// Returns the placeholder names detected in `template` without resolving any
/// of them.
///
/// # Arguments
///
/// * `template` - Template text such as `"{a} + {b}"` to inspect.
///
/// # Returns
///
/// Ordered list of placeholder names found in `template`.
pub(crate) fn list_template_placeholders(template: &str) -> Vec<String> {
    let params = std::collections::HashMap::<String, String>::new();
    let (_resolved, missing_keys) = query_helpers::replace_template_placeholders(template, &params);
    missing_keys
}

/// Parses and evaluates `expression` as a math expression with no variables
/// bound.
///
/// # Arguments
///
/// * `expression` - Math expression source such as `"a + b * 2"`.
///
/// # Errors
///
/// Returns an error when the expression fails to parse or evaluates to a
/// non-finite value (NaN, infinity).
pub(crate) fn evaluate_math_expression(expression: &str) -> Result<f64> {
    let evaluated_value = evalexpr::eval_number(expression)
        .with_context(|| format!("Failed to evaluate math expression `{}`", expression))?;

    if !evaluated_value.is_finite() {
        bail!(
            "Math expression `{}` returned a non-finite value",
            expression
        );
    }

    Ok(evaluated_value)
}

/// Renders `value` as a string, dropping trailing fractional zeros and using
/// an integer format when the value is within `1e-9` of an integer.
///
/// # Arguments
///
/// * `value` - Numeric result to render.
pub(crate) fn format_math_result(value: f64) -> String {
    let normalized_value = if value.abs() < 1e-12 { 0.0 } else { value };
    let rounded_value = normalized_value.round();

    if (normalized_value - rounded_value).abs() < 1e-9 {
        return format!("{rounded_value:.0}");
    }

    let mut rendered = format!("{normalized_value:.12}");
    while rendered.contains('.') && rendered.ends_with('0') {
        rendered.pop();
    }

    if rendered.ends_with('.') {
        rendered.pop();
    }

    rendered
}
