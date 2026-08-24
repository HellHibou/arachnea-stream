use super::super::post_processes;
use tracing::warn;

/// Applies the `math_formula` scraper action by evaluating a math expression
/// for every value in the current value list.
///
/// The `formula` template supports a `{value}` placeholder which is replaced
/// by each current value before evaluation.
///
/// # Arguments
///
/// * `texts` - Math expression template, e.g. `"{value} * 2 + 5"`.
///
/// # Returns
///
/// A new value list where each entry is the evaluated and formatted result.
pub(super) fn apply(texts: Vec<String>) -> Vec<String> {
    let mut new_texts: Vec<String> = Vec::new();

    for expression in texts {
        match post_processes::evaluate_math_expression(&expression) {
            Ok(result) => {
                new_texts.push(post_processes::format_math_result(result));
            }
            Err(error) => {
                warn!("Can't execute {}: {}", expression, error);
            }
        }
    }

    new_texts
}
