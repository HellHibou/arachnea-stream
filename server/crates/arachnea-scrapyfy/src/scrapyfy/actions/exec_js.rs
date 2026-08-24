use anyhow::Result;
use boa_engine::{
    context::ContextBuilder, js_string, property::Attribute, realm::Realm, Context, JsObject,
    JsValue, NativeFunction, Source,
};
use regex::Regex;
use std::cell::RefCell;
use std::rc::Rc;
use tracing::trace;

const MAX_SCRIPTS: usize = 50;
const DEFAULT_LOOP_LIMIT: u64 = 10_000_000;

/// Wraps a Boa JS [`Context`] together with the synthetic `document` and `window`
/// global objects and the inline buffer used by `document.write` / `document.writeln`.
#[allow(dead_code)]
struct JsContext {
    /// The Boa JavaScript engine context.
    ctx: Context,
    /// Synthetic `document` object exposing `write` and `writeln`.
    document: JsObject,
    /// Synthetic `window` global object (currently empty, prevents crashes from
    /// code that references `window` unconditionally).
    window: JsObject,
    /// Buffer that accumulates content passed to `document.write` / `writeln`
    /// during JS execution.
    document_content: Rc<RefCell<String>>,
}

/// Evaluate a list of JS text snippets and extract numeric variables that were
/// created during execution, together with any content written via
/// `document.write` / `document.writeln`.
///
/// # Arguments
///
/// * `texts` - JS source strings to evaluate.
/// * `timeout_ms` - Approximate timeout that is translated into a loop iteration
///   limit on the Boa context.
/// * `response_body` - Optional HTML body from which `<script>` contents can be
///   extracted when `inject_html_scripts` is true.
/// * `inject_html_scripts` - If true, extract inline scripts from `response_body`
///   and prepend them to the sources.
/// * `scripts` - Hardcoded JS snippets that run before any other source.
///
/// # Returns
///
/// A list of `"key=value"` strings for every new numeric global variable, plus a
/// `"document_write=..."` entry if `document.write` / `writeln` was called.
pub(super) fn apply(
    texts: Vec<String>,
    timeout_ms: u64,
    response_body: Option<&str>,
    inject_html_scripts: bool,
    scripts: &[String],
) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let mut js_ctx = build_context(timeout_ms)?;
    let mut all_sources = Vec::<String>::new();
    all_sources.extend(scripts.iter().cloned());

    if inject_html_scripts {
        if let Some(html) = response_body {
            all_sources.extend(extract_html_scripts(html));
        }
    }

    for source in all_sources.iter().take(MAX_SCRIPTS) {
        trace!("Execute {}", source);
        exec_js_source(&mut js_ctx.ctx, source)?;
    }

    for text in &texts {
        let exec_result = exec_js_source(&mut js_ctx.ctx, text)?;
        trace!("{} return {}", text, exec_result);
        result.push(exec_result);
    }

    Ok(result)
}

/// Build a [`JsContext`] with a fresh Boa [`Context`], loop iteration limit based
/// on `timeout_ms`, and registered `document` / `window` global objects.
fn build_context(timeout_ms: u64) -> Result<JsContext> {
    let mut ctx = ContextBuilder::new()
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to create JS context: {}", e))?;

    let loop_limit = (timeout_ms as u64) * 20_000;
    ctx.runtime_limits_mut()
        .set_loop_iteration_limit(loop_limit.max(DEFAULT_LOOP_LIMIT));

    let document_content = Rc::new(RefCell::new(String::new()));

    let write_fn = create_document_write_fn(Rc::clone(&document_content), false, ctx.realm());
    let writeln_fn = create_document_write_fn(Rc::clone(&document_content), true, ctx.realm());

    let document = JsObject::with_object_proto(ctx.intrinsics());

    document
        .set(js_string!("write"), write_fn, false, &mut ctx)
        .map_err(|e| anyhow::anyhow!("Failed to set document.write: {}", e))?;

    document
        .set(js_string!("writeln"), writeln_fn, false, &mut ctx)
        .map_err(|e| anyhow::anyhow!("Failed to set document.writeln: {}", e))?;

    ctx.register_global_property(js_string!("document"), document.clone(), Attribute::all())
        .map_err(|e| anyhow::anyhow!("Failed to register document: {}", e))?;

    let window = JsObject::with_object_proto(ctx.intrinsics());

    ctx.register_global_property(js_string!("window"), window.clone(), Attribute::all())
        .map_err(|e| anyhow::anyhow!("Failed to register window: {}", e))?;

    Ok(JsContext {
        ctx,
        document,
        window,
        document_content,
    })
}

/// Create a `JsFunction` that appends joined arguments to a shared buffer,
/// optionally adding a trailing newline (for `document.writeln`).
///
/// # Safety
///
/// Uses `NativeFunction::from_closure` because the closure captures an
/// `Rc<RefCell<String>>` which is not traceable by the Boa GC. This is safe
/// because the captured value is a plain Rust allocation whose lifetime is
/// tied to the [`JsContext`].
fn create_document_write_fn(
    buffer: Rc<RefCell<String>>,
    append_newline: bool,
    realm: &Realm,
) -> boa_engine::object::builtins::JsFunction {
    unsafe {
        NativeFunction::from_closure(
            move |_this: &JsValue, args: &[JsValue], context: &mut Context| {
                let mut output = String::new();
                for arg in args {
                    let s = arg.to_string(context)?;
                    output.push_str(&s.to_std_string_escaped());
                }
                if append_newline {
                    output.push('\n');
                }
                buffer.borrow_mut().push_str(&output);
                Ok(JsValue::undefined())
            },
        )
    }
    .to_js_function(realm)
}

/// Extract the content of all `<script>…</script>` tags from an HTML string.
fn extract_html_scripts(html: &str) -> Vec<String> {
    let script_re = Regex::new(r"(?is)<script[^>]*>(.*?)</script>").unwrap();
    script_re
        .captures_iter(html)
        .map(|c| c[1].to_string())
        .filter(|s| s.contains("eval") || s.contains("function") || !s.trim().is_empty())
        .collect()
}

/// Evaluate a JS source string in the given Boa context and return the resulting
/// value as a [`String`].
///
/// # Errors
///
/// Returns an error if the engine rejects the source (parse or runtime error).
fn exec_js_source(ctx: &mut Context, source: &str) -> Result<String> {
    let value = ctx
        .eval(Source::from_bytes(source))
        .map_err(|e| anyhow::anyhow!("JS execution error: {}\n{}", e, source))?;
    value
        .to_string(ctx)
        .map(|s| s.to_std_string_escaped())
        .map_err(|e| anyhow::anyhow!("JS value to string error: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_js_execution() {
        let texts = vec!["var x = 42;".to_string()];
        let result = apply(texts, 500, None, false, &[]).unwrap();
        assert!(result.contains(&"x=42".to_string()));
    }

    #[test]
    fn test_text_is_js_not_html() {
        let texts = vec!["<html><script>var a = 1;</script></html>".to_string()];
        let result = apply(texts, 500, None, false, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_inject_html_scripts_extracts_from_response_body() {
        let texts = vec!["".to_string()];
        let html = "<html><script>var a = 99;</script></html>";
        let result = apply(texts, 500, Some(html), true, &[]).unwrap();
        assert!(result.contains(&"a=99".to_string()));
    }

    #[test]
    fn test_hardcoded_scripts_are_injected() {
        let texts = vec!["var y = helper();".to_string()];
        let result = apply(
            texts,
            500,
            None,
            false,
            &["function helper() { return 42; }".to_string()],
        )
        .unwrap();
        assert!(result.contains(&"y=42".to_string()));
    }

    #[test]
    fn test_inject_html_plus_hardcoded_plus_text() {
        let texts = vec!["var z = x + y;".to_string()];
        let html = "<html><script>var x = 10;</script></html>";
        let result = apply(texts, 500, Some(html), true, &["var y = 5;".to_string()]).unwrap();
        assert!(result.contains(&"x=10".to_string()));
        assert!(result.contains(&"y=5".to_string()));
        assert!(result.contains(&"z=15".to_string()));
    }

    #[test]
    fn test_numeric_variable_detection() {
        let texts = vec!["var score = 100; var name = 'alice';".to_string()];
        let result = apply(texts, 500, None, false, &[]).unwrap();
        assert!(result.contains(&"score=100".to_string()));
        assert!(!result.iter().any(|line| line.starts_with("name=")));
    }

    #[test]
    fn test_xor_evaluation() {
        let texts = vec!["var Eight4EightNine = 88 ^ 0;".to_string()];
        let result = apply(texts, 500, None, false, &[]).unwrap();
        assert!(result.contains(&"Eight4EightNine=88".to_string()));
    }

    #[test]
    fn test_transitive_resolution() {
        let texts = vec![
            "Six0Eight = 0; ZeroThreeSeven = Six0Eight ^ 0; Eight4EightNine = ZeroThreeSeven ^ 88;"
                .to_string(),
        ];
        let result = apply(texts, 500, None, false, &[]).unwrap();
        assert!(result.contains(&"Eight4EightNine=88".to_string()));
    }

    #[test]
    fn test_loop_limit_does_not_block() {
        let texts = vec!["var quick = 1;".to_string()];
        let result = apply(texts, 5000, None, false, &[]).unwrap();
        assert!(result.contains(&"quick=1".to_string()));
    }

    #[test]
    fn test_empty_input() {
        let texts: Vec<String> = vec![];
        let result = apply(texts, 500, None, false, &[]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_spysone_simulation() {
        let html =
            r#"<html><script>ZeroThreeSeven=0;Eight4EightNine=ZeroThreeSeven^88;</script></html>"#;
        let texts = vec!["".to_string()];
        let result = apply(texts, 500, Some(html), true, &[]).unwrap();
        assert!(result
            .iter()
            .any(|line| line.starts_with("Eight4EightNine")));
    }

    #[test]
    fn test_configured_scripts_run_before_html_scripts() {
        let html = r#"<html><script>var result = (typeof document !== 'undefined' && typeof document.write === 'function') ? 1 : 0;</script></html>"#;
        let texts = vec!["var spysone_port = result + 99;".to_string()];
        let result = apply(
            texts,
            500,
            Some(html),
            true,
            &["var document = { write: function(s) {} };".to_string()],
        )
        .unwrap();
        assert!(result.iter().any(|line| line == "spysone_port=100"));
    }

    #[test]
    fn test_configured_script_provides_document_write_html_has_packer_vars() {
        let packer_script =
            "Eight4EightNine=90^0;TwoSevenFive=60^0;Seven4SixZero=47^0;SixNineFour=27^0;";
        let html = format!(r#"<html><script>{}</script></html>"#, packer_script);
        let texts = vec![
            "var spysone_port = (Eight4EightNine ^ TwoSevenFive) + (Seven4SixZero ^ SixNineFour);"
                .to_string(),
        ];
        let result = apply(
            texts,
            500,
            Some(&html),
            true,
            &["var document = { write: function(s) { return s; } };".to_string()],
        )
        .unwrap();
        assert!(result.iter().any(|line| line == "Eight4EightNine=90"));
        assert!(result.iter().any(|line| line == "TwoSevenFive=60"));
        assert!(result.iter().any(|line| line == "Seven4SixZero=47"));
        assert!(result.iter().any(|line| line == "SixNineFour=27"));
        // (90 ^ 60) + (47 ^ 27) = 102 + 52 = 154
        assert!(result.iter().any(|line| line == "spysone_port=154"));
    }

    #[test]
    fn test_window_stub_prevents_ga_crash() {
        let html = r#"<html><script>window.dataLayer = window.dataLayer || []; function gtag(){dataLayer.push(arguments);} gtag('js', new Date()); gtag('config', 'G-XWX5S73YKH');</script><script>var Eight4EightNine = 88 ^ 0;</script></html>"#;
        let texts = vec!["var spysone_port = Eight4EightNine + 0;".to_string()];
        let result = apply(
            texts,
            500,
            Some(html),
            true,
            &[
                "var window = {}; var dataLayer = []; var document = { write: function(s) {} };"
                    .to_string(),
            ],
        )
        .unwrap();
        assert!(result.iter().any(|line| line == "Eight4EightNine=88"));
        assert!(result.iter().any(|line| line == "spysone_port=88"));
    }
}
