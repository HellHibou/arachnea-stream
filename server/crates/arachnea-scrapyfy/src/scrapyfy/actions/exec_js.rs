use anyhow::Result;
use boa_engine::{
    context::ContextBuilder, js_string, property::Attribute, realm::Realm, Context, JsObject,
    JsValue, NativeFunction, Source,
};
use regex::Regex;
use std::cell::RefCell;
use std::rc::Rc;
use tracing::trace;

use crate::JS_MAX_SCRIPTS as MAX_SCRIPTS;

use crate::JS_DEFAULT_LOOP_LIMIT as DEFAULT_LOOP_LIMIT;

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
    }

    let mut result = exec_js_source(
        &mut js_ctx.ctx,
        "Object.keys(globalThis)\n            .filter((key) => typeof globalThis[key] === 'number')\n            .map((key) => `${key}=${globalThis[key]}`)\n            .join('\\n')",
    )?
    .lines()
    .filter(|line| !line.is_empty())
    .map(str::to_owned)
    .collect::<Vec<_>>();

    let document_content = js_ctx.document_content.borrow();
    if !document_content.is_empty() {
        result.push(format!("document_write={document_content}"));
    }

    Ok(result)
}

/// Build a [`JsContext`] with a fresh Boa [`Context`], loop iteration limit based
/// on `timeout_ms`, and registered `document` / `window` global objects.
///
/// # Arguments
///
/// * `timeout_ms` - Maximum JavaScript execution duration in milliseconds.
fn build_context(timeout_ms: u64) -> Result<JsContext> {
    let mut ctx = ContextBuilder::new()
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to create JS context: {}", e))?;

    let loop_limit = (timeout_ms as u64) * crate::JS_LOOP_ITERATIONS_PER_MS;
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
/// # Arguments
///
/// * `buffer` - Shared output buffer receiving simulated `document.write` calls.
/// * `append_newline` - Whether a write appends a newline, as with `document.writeln`.
/// * `realm` - JavaScript realm used to allocate the native write callback.
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
///
/// # Arguments
///
/// * `html` - HTML source from which inline script bodies are extracted.
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
/// # Arguments
///
/// * `ctx` - JavaScript context in which the supplied source is evaluated.
/// * `source` - JavaScript source evaluated in the provided context.
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
#[path = "../../../tests/scrapyfy/actions/exec_js.rs"]
mod tests;
