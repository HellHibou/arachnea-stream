//! Typed facade adapters; only the outer payload container is normalized.

use super::{
    source_checks::{Operation, SourceCheck},
    StreamScraper,
};
use anyhow::{ensure, Result};
use arachnea_core::controler::RequestControlerContext;
use arachnea_scrapyfy::scrapyfy::scraper_result::ScraperAggregationResult;
use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use std::{collections::HashMap, sync::LazyLock};

/// Failure classification retains the original cause for sanitized reporting.
pub(super) struct Failure {
    /// Stable source-check outcome; opaque backend failures remain contract failures.
    pub outcome: &'static str,
    /// Original contextual error chain.
    pub error: anyhow::Error,
}

impl From<anyhow::Error> for Failure {
    fn from(error: anyhow::Error) -> Self {
        let transport = error.chain().any(|cause| {
            cause.downcast_ref::<rquest::Error>().is_some_and(|error| {
                error.is_connect()
                    || error.is_timeout()
                    || error.is_status()
                    || error.is_request()
                    || error.is_body()
                    || error.is_redirect()
            }) || cause.downcast_ref::<std::io::Error>().is_some_and(|error| {
                matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::ConnectionRefused
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::NotConnected
                )
            })
        });
        Self {
            outcome: if transport {
                "retrieval_failed"
            } else {
                "contract_failed"
            },
            error,
        }
    }
}

/// Executes one recognized operation with a fresh context and the selected source.
///
/// # Arguments
/// * `scraper` - Facade containing exactly the selected source.
/// * `contract` - Validated source identity and expectations.
/// * `operation` - Generic facade adapter to execute.
/// * `inputs` - Resolved string inputs and optional page.
///
/// # Errors
/// Preserves transport causes and fails on any envelope error or foreign source.
pub(super) async fn execute(
    scraper: &StreamScraper,
    contract: &SourceCheck,
    operation: Operation,
    inputs: &HashMap<String, Value>,
) -> std::result::Result<Vec<Value>, Failure> {
    let context = RequestControlerContext::default();
    if context.get_header("If-None-Match").is_some() {
        return Err(anyhow::anyhow!("Source-check context contains a client validator").into());
    }
    println!(
        "source={} operation={} client_validator=none",
        contract.source,
        operation.name()
    );
    let source = contract.source.clone();
    let string = |key: &str| -> Result<String> {
        inputs
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("Missing resolved string input `{key}`"))
    };
    let page = inputs.get("page").and_then(Value::as_u64).unwrap_or(1) as usize;
    let roots = match operation {
        Operation::ServiceStreamMetadata => {
            let (result, _) = scraper.get_service(context).await?;
            let roots = envelope(result, false, &source, operation)?;
            validate_metadata(scraper, &source, &roots).await?;
            roots
        }
        Operation::LoadHome => envelope(
            scraper
                .load_home(context, Some(vec![source.clone()]))
                .await?
                .0,
            false,
            &source,
            operation,
        )?,
        Operation::Search => envelope(
            scraper
                .search(
                    context,
                    string("query")?,
                    vec![],
                    vec![],
                    1,
                    Default::default(),
                )
                .await?
                .0,
            false,
            &source,
            operation,
        )?,
        Operation::GetCategory => envelope(
            scraper
                .get_category(
                    context,
                    vec![HashMap::from([
                        ("name".to_string(), source.clone()),
                        ("query_url".to_string(), string("query_url")?),
                    ])],
                    page,
                    Default::default(),
                )
                .await?
                .0,
            false,
            &source,
            operation,
        )?,
        Operation::GetSection => envelope(
            scraper
                .get_section(
                    context,
                    source.clone(),
                    string("query_url")?,
                    page,
                    Default::default(),
                )
                .await?
                .0,
            true,
            &source,
            operation,
        )?,
        Operation::GetEntry => envelope(
            scraper
                .get_entry(context, source.clone(), string("query_url")?)
                .await?
                .0,
            true,
            &source,
            operation,
        )?,
        Operation::GetPlayers => envelope(
            scraper
                .get_players(context, source.clone(), string("query_url")?)
                .await?
                .0,
            true,
            &source,
            operation,
        )?,
        Operation::GetSeason => envelope(
            scraper
                .get_season(context, source.clone(), string("query_url")?, page)
                .await?
                .0,
            true,
            &source,
            operation,
        )?,
        Operation::ListLives => envelope(
            scraper.list_lives(context).await?.0,
            false,
            &source,
            operation,
        )?,
        Operation::GetLive => envelope(
            scraper
                .get_live(context, source.clone(), string("query_url")?)
                .await?
                .0,
            true,
            &source,
            operation,
        )?,
        Operation::GetBanners => envelope(
            scraper
                .get_banners(context, source.clone(), string("query_url")?)
                .await?
                .0,
            true,
            &source,
            operation,
        )?,
        Operation::GetRecommendations => envelope(
            scraper
                .get_recommendations(
                    context,
                    source.clone(),
                    string("query_url")?,
                    page,
                    Default::default(),
                )
                .await?
                .0,
            true,
            &source,
            operation,
        )?,
        Operation::ResolveStream => envelope(
            scraper
                .get_stream(
                    crate::SCRAPER_QUERY_STREAM_RESOLVER_ID.to_string(),
                    string("query_url")?,
                    Some(source.clone()),
                    vec![],
                    Some(false),
                )
                .await?,
            true,
            &source,
            operation,
        )?,
        Operation::GetEpisode => {
            // No typed facade exists for this legacy production query.
            let mut params = HashMap::from([
                ("query_url".to_string(), string("query_url")?),
                ("link".to_string(), string("query_url")?),
            ]);
            scraper.enrich_runtime_params(&mut params);
            let sources = vec![source.clone()];
            envelope(
                scraper
                    .scraper_agregator
                    .execute_query_async(
                        &context,
                        Default::default(),
                        crate::STREAM_SERVICE_GROUP_NAME,
                        operation.name(),
                        &params,
                        arachnea_scrapyfy::ScraperAggregationOptions {
                            source_params: None,
                            scrapper_list: Some(&sources),
                            query_media_type_filter: None,
                            fields_filters: None,
                            source_field_name: Some("source"),
                            operation: operation.name(),
                        },
                    )
                    .await,
                false,
                &source,
                operation,
            )?
        }
    };
    for root in &roots {
        validate_source(root, &source)?;
    }
    Ok(roots)
}

fn envelope<T: Serialize>(
    result: ScraperAggregationResult<T>,
    singleton: bool,
    source: &str,
    operation: Operation,
) -> Result<Vec<Value>> {
    for (validated_source, validation) in &result.validations {
        println!("source={source} operation={} validated_source={validated_source} validation_status={:?}", operation.name(), validation.status);
    }
    ensure!(
        result.errors.is_empty(),
        "Facade envelope failed: {}",
        result
            .errors
            .iter()
            .map(|error| format!(
                "code={} operation={} source={:?}: {}",
                error.code, error.operation, error.source, error.message
            ))
            .collect::<Vec<_>>()
            .join("; ")
    );
    let payload = serde_json::to_value(result.data)?;
    if singleton {
        Ok(vec![payload])
    } else {
        payload
            .as_array()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Facade vector result is not an array"))
    }
}

fn validate_source(value: &Value, source: &str) -> Result<()> {
    match value {
        Value::Object(object) => {
            if let Some(found) = object.get("source") {
                ensure!(
                    found.as_str() == Some(source),
                    "Foreign or malformed source in public payload"
                );
            }
            for child in object.values() {
                validate_source(child, source)?;
            }
        }
        Value::Array(array) => {
            for child in array {
                validate_source(child, source)?;
            }
        }
        _ => (),
    }
    Ok(())
}

/// Preserves the existing metadata/filter/deserialization assertions without provider requests.
async fn validate_metadata(scraper: &StreamScraper, source: &str, roots: &[Value]) -> Result<()> {
    ensure!(
        roots.len() == 1 && roots[0].get("id").and_then(Value::as_str) == Some(source),
        "Metadata must identify exactly the selected source"
    );
    let advertised = roots[0]
        .get("supported_queries")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("Missing advertised capabilities"))?;
    let configured = scraper
        .scraper_agregator
        .query_services()
        .filter(|group| group.name == crate::STREAM_SERVICE_GROUP_NAME)
        .flat_map(|group| &group.services)
        .find(|collection| collection.name() == source)
        .map(|collection| collection.query_names())
        .ok_or_else(|| anyhow::anyhow!("Selected collection is not loaded"))?;
    ensure!(
        advertised.len() == configured.len()
            && configured
                .iter()
                .all(|name| advertised.contains(&Value::String(name.clone()))),
        "Advertised capabilities differ from the selected collection"
    );
    let (empty, etag) = scraper
        .load_home(RequestControlerContext::default(), Some(vec![]))
        .await?;
    ensure!(
        empty.data.is_empty() && empty.errors.is_empty() && etag.is_none(),
        "Empty home selection contract changed"
    );
    let unknown_source = format!("{source}-unknown-source-for-filter-validation");
    let (unknown, etag) = scraper
        .load_home(
            RequestControlerContext::default(),
            Some(vec![unknown_source.clone()]),
        )
        .await?;
    ensure!(
        unknown.data.is_empty()
            && unknown.errors.len() == 1
            && etag.is_none()
            && unknown.errors[0].source.as_deref() == Some(unknown_source.as_str()),
        "Unknown home selection contract changed"
    );
    for invalid in [
        r#"{"sources":null}"#,
        r#"{"sources":"source-for-filter-validation"}"#,
        r#"{"sources":[""]}"#,
        r#"{"sources":[" "]}"#,
    ] {
        ensure!(
            serde_json::from_str::<super::super::LoadHomeRequest>(invalid).is_err(),
            "Invalid home filter accepted"
        );
    }
    ensure!(
        serde_json::from_str::<super::super::LoadHomeRequest>("{}")?
            .sources
            .is_none(),
        "Omitted home filter changed"
    );
    ensure!(
        serde_json::from_str::<super::super::LoadHomeRequest>(r#"{"sources":[]}"#)?.sources
            == Some(vec![]),
        "Explicit empty home filter changed"
    );
    Ok(())
}

/// Produces a comparable reference fingerprint without exposing URL paths or credentials.
pub(super) fn reference(value: &str) -> String {
    let fingerprint = arachnea_core::crypt::hash62_64(value.as_bytes());
    if let Ok(url) = url::Url::parse(value) {
        if let Some(host) = url.host_str() {
            return format!(
                "url(scheme={},host={host},reference={fingerprint})",
                url.scheme()
            );
        }
    }
    format!("<redacted>#{fingerprint}")
}

/// Redacts URLs and credential-like tokens from technical error chains.
pub(super) fn sanitize(message: &str) -> String {
    static URLS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"(?i)https?://[^\s\"'<>]+"#).unwrap());
    static SECRETS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)(authorization\s*[:=]\s*(?:bearer\s+|basic\s+)?|bearer\s+|(?:access_token|token|password|secret)\s*[:=]\s*)[^\s,;]+")
            .unwrap()
    });
    let urls = URLS.replace_all(message, |captures: &regex::Captures<'_>| {
        reference(&captures[0])
    });
    SECRETS.replace_all(&urls, "${1}<redacted>").into_owned()
}
