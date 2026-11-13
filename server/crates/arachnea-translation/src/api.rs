//! Controller route registration for translation integrations.
use arachnea_core::controler::{ControlerService, ControlerServiceExt};
use serde::Deserialize;
use std::sync::Arc;

/// Dependencies borrowed from one retained application snapshot.
///
/// All accessors must describe the same immutable catalog and provider runtime.
/// The API retains the snapshot until the request has completed.
pub trait TranslationSnapshot: Send + Sync + 'static {
    /// Returns the validated provider catalog belonging to this snapshot.
    fn catalog(&self) -> &crate::catalog::TranslationCatalog;
    /// Returns the loaded provider aggregator matching the catalog revision.
    fn aggregator(&self) -> &arachnea_scrapyfy::ScraperAgregator;
    /// Returns the configured Scrapyfy group containing translation providers.
    fn group_name(&self) -> &str;
    /// Creates an authorized retriever configured from this snapshot.
    ///
    /// This is called only for document requests; catalog requests do not need it.
    fn retrieval(&self) -> Box<dyn crate::document::SubtitleRetrievalPort>;
    /// Returns the trusted public proxy prefix recognized by the retriever.
    fn proxy_public_path(&self) -> Option<&str> {
        None
    }
}

#[derive(Deserialize)]
struct EmptyRequest {}

/// Registers translation commands and builds their handlers from a fresh snapshot.
///
/// The factory is invoked once per request. Its returned `Arc` keeps dependencies
/// alive throughout asynchronous execution, even if the application reloads.
///
/// # Arguments
/// * `controller` - Target controller.
/// * `snapshot` - Factory returning one retained application snapshot per request.
/// * `subpath` - Optional slash-separated route prefix.
///
/// # Errors
/// Returns an error for empty interior, dot, query or fragment segments.
pub fn register_routes<S, F>(
    controller: &mut dyn ControlerService,
    snapshot: F,
    subpath: Option<&str>,
) -> Result<(), String>
where
    S: TranslationSnapshot,
    F: Fn() -> Arc<S> + Send + Sync + 'static,
{
    let snapshot = Arc::new(snapshot);
    let prefix = subpath.unwrap_or("").trim_matches('/');
    if prefix.contains(['?', '#'])
        || (!prefix.is_empty()
            && prefix
                .split('/')
                .any(|segment| segment.is_empty() || segment == "." || segment == ".."))
    {
        return Err("invalid translation route prefix".into());
    }
    let route = |command: &str| {
        if prefix.is_empty() {
            command.to_string()
        } else {
            format!("{prefix}/{command}")
        }
    };
    controller.register_result_function_with_state(
        &route("get_services"),
        Arc::clone(&snapshot),
        |factory, _, _: EmptyRequest| async move {
            let snapshot = factory();
            serde_json::to_value(TranslationServicesView::from_catalog(snapshot.catalog()))
                .map(|value| (value, None::<String>))
                .map_err(|error| error.to_string())
        },
    );
    controller.register_stream_function_with_state(
        &route("translate_url"),
        snapshot,
        |factory, input| async move {
            let snapshot = factory();
            let retrieval = snapshot.retrieval();
            let handler = TranslationHandler {
                translation_catalog: snapshot.catalog(),
                aggregator: snapshot.aggregator(),
                group_name: snapshot.group_name(),
                retrieval: retrieval.as_ref(),
                proxy_public_path: snapshot.proxy_public_path(),
            };
            handler
                .get_translated_subtitle(input)
                .await
                .map_err(|error| error.to_string())
        },
    );
    Ok(())
}

use crate::{
    document::{error::SubtitleTranslationError, types::SubtitleFormat},
    SelectionPolicy,
};

/// Parameters of one direct conditional subtitle translation request.
#[derive(Debug, Deserialize)]
pub struct TranslateSubtitleRequest {
    /// Source subtitle URL or recognized application proxy link.
    pub url: String,
    /// Stable source track identifier when the caller already knows one.
    #[serde(default)]
    pub track_id: Option<String>,
    /// Known source language, or omitted for automatic detection.
    #[serde(default)]
    pub source: Option<String>,
    /// Requested target language.
    pub target: String,
    /// Automatic, preferred or constrained service policy.
    #[serde(default)]
    pub policy: Option<SelectionPolicy>,
    /// Source subtitle format; only WebVTT is accepted initially.
    #[serde(rename = "content-type")]
    pub content_type: String,
}

/// Parses a requested MIME type and resolves its compiled document translator.
///
/// # Arguments
/// * `value` - Required MIME type, optionally including parameters.
///
/// # Errors
/// Returns an error when the type is unsupported.
pub fn parse_format(value: &str) -> Result<SubtitleFormat, SubtitleTranslationError> {
    let media_type = value.split(';').next().unwrap_or(value).trim();
    crate::document::translator(media_type)?;
    SubtitleFormat::from_content_type(media_type)
        .filter(|format| format.is_initial_delivery_supported())
        .ok_or_else(|| SubtitleTranslationError::UnsupportedFormat {
            media_type: media_type.to_string(),
        })
}

/// Decodes a document request query using the JSON command parameter convention.
///
/// # Arguments
/// * `query` - URL encoded query string.
///
/// # Errors
/// Returns an error for duplicate or invalid parameters.
pub fn parse_request(query: &str) -> Result<TranslateSubtitleRequest, SubtitleTranslationError> {
    let mut parameters = serde_json::Map::new();
    for (name, value) in url::form_urlencoded::parse(query.as_bytes()) {
        let value = serde_json::from_str(&value)
            .unwrap_or_else(|_| serde_json::Value::String(value.into_owned()));
        if parameters.insert(name.into_owned(), value).is_some() {
            return Err(SubtitleTranslationError::InvalidDescriptor {
                detail: "duplicate subtitle translation parameter".to_string(),
            });
        }
    }
    serde_json::from_value(serde_json::Value::Object(parameters)).map_err(|_| {
        SubtitleTranslationError::InvalidDescriptor {
            detail: "invalid subtitle translation parameters".to_string(),
        }
    })
}

mod handler;
pub use handler::{TranslationHandler, TranslationServicesView};
