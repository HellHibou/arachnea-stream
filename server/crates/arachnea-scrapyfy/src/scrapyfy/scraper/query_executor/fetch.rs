//! Request resolution, conditional validation and HTTP fetching.

use super::*;

/// Fetches a single URL using the configured HTTP method, headers, and body.
///
/// Dispatches on `extract_next_data` for JSON queries; HTML always returns
/// the response body as text.
///
/// # Arguments
///
/// * `client` - The HTTP client to use for the request.
/// * `method` - HTTP method (GET or POST).
/// * `url` - The URL to fetch.
/// * `headers` - HTTP headers to include in the request.
/// * `body` - Optional request body.
/// * `extract_next_data` - Whether to parse the response as Next.js `__NEXT_DATA__`.
/// * `scraper_type` - The type of scraper (HTML, JSON, or Static).
/// * `pre_processes` - Response transformations declared by the query.
/// * `query_name` - Query name used in pre-processing diagnostics.
/// * `base_url` - Source base URL exposed as `{base_url}` to pre-processing templates.
/// * `params` - Runtime parameters exposed to pre-processing templates.
///
/// # Returns
///
/// A [`FetchedResponse`] containing the parsed response data.
///
/// # Errors
///
/// Returns an error if the HTTP request fails.
pub(super) async fn fetch_single(
    client: &HttpClient,
    method: http::Method,
    url: &str,
    headers: &HashMap<String, String>,
    body: Option<&str>,
    extract_next_data: bool,
    scraper_type: ScraperType,
    empty_on_statuses: &[u16],
    pre_processes: &[PreProcessAction],
    query_name: &str,
    base_url: &str,
    params: &HashMap<String, String>,
) -> Result<FetchedResponse> {
    let response = client.send_for_request(method, url, headers, body).await?;
    if empty_on_statuses.contains(&response.status().as_u16()) {
        return Ok(empty_response(scraper_type));
    }
    let text = response.text().await?;

    match scraper_type {
        ScraperType::Json => {
            let text = apply_pre_processes(text, pre_processes, query_name, base_url, params)?;
            if extract_next_data {
                Ok(FetchedResponse::Json(HttpClient::extract_next_data_json(
                    url, &text,
                )?))
            } else {
                Ok(FetchedResponse::Json(serde_json::from_str(&text).map_err(
                    |error| anyhow::anyhow!("Invalid JSON payload returned by {url}: {error}"),
                )?))
            }
        }
        ScraperType::Html => {
            let html = apply_pre_processes(text, pre_processes, query_name, base_url, params)?;
            Ok(FetchedResponse::Html(html))
        }
        ScraperType::Text => {
            let text = apply_pre_processes(text, pre_processes, query_name, base_url, params)?;
            Ok(FetchedResponse::Text(text))
        }
        ScraperType::Static => Ok(FetchedResponse::Static),
    }
}

pub(super) fn empty_response(scraper_type: ScraperType) -> FetchedResponse {
    match scraper_type {
        ScraperType::Json => FetchedResponse::Json(Value::Array(Vec::new())),
        ScraperType::Html => FetchedResponse::Html(String::new()),
        ScraperType::Text => FetchedResponse::Text(String::new()),
        ScraperType::Static => FetchedResponse::Static,
    }
}

pub(super) fn empty_response_body(scraper_type: ScraperType) -> &'static str {
    match scraper_type {
        ScraperType::Json => "[]",
        ScraperType::Html | ScraperType::Text | ScraperType::Static => "",
    }
}

/// Applies the incoming conditional validators as request headers.
///
/// # Arguments
///
/// * `headers` - Header map mutated in place.
/// * `conditional` - Validators derived from the client fragment.
pub(super) fn apply_conditional_headers(
    headers: &mut HashMap<String, String>,
    conditional: &ConditionalRequest,
) {
    if let Some(if_none_match) = &conditional.if_none_match {
        headers.insert("if-none-match".to_string(), if_none_match.clone());
    }
    if let Some(if_modified_since) = &conditional.if_modified_since {
        headers.insert("if-modified-since".to_string(), if_modified_since.clone());
    }
}

/// Fetches the root response of one source under conditional validation.
///
/// Sends the conditional headers derived from the client fragment, detects a
/// `304 Not Modified` or an identical content hash, records the up-to-date
/// validator fragment into the shared slot, and returns a
/// [`FetchedResponse::NotModified`] marker when parsing can be skipped.
///
/// # Arguments
///
/// * `query` - The root query being executed.
/// * `urls` - Resolved request URLs; the first non-empty one is validated.
/// * `context` - Runtime context containing parameters and HTTP client.
/// * `validation` - Shared slot carrying the incoming fragment and outcome.
///
/// # Returns
///
/// A single-element vector with either the parsed response or a
/// [`FetchedResponse::NotModified`] marker.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the response cannot be parsed.
pub(super) async fn fetch_responses_with_validation(
    query: &dyn ScraperQuery,
    urls: &[String],
    context: &QueryContext<'_>,
    validation: &ValidationSlot,
) -> Result<Vec<(String, FetchedResponse)>> {
    let url = urls
        .iter()
        .find(|url| !url.trim().is_empty())
        .cloned()
        .unwrap_or_default();
    if url.is_empty() {
        return Ok(Vec::new());
    }

    let method = query.request_method().as_http_method();
    let mut headers = resolve_request_headers(
        query.request_headers(),
        context.params,
        &url,
        context.parent_response,
    );
    let body = resolve_request_body(
        query.request_body_pointer(),
        query.request_body_select(),
        query.request_body_actions(),
        context.params,
        &url,
        context.parent_response,
    );
    let incoming = validation.request().cloned().unwrap_or_default();
    apply_conditional_headers(&mut headers, &incoming);

    let record_outcome = |not_modified: bool, etag_fragment: String| {
        validation.record(RootFetchOutcome {
            not_modified,
            etag_fragment,
        });
    };

    // Browser-driven executions cannot send conditional headers reliably;
    // fall back to content-hash comparison on the rendered page.
    if matches!(
        query.http_config().execution,
        Some(ScraperHttpExecution::PageNavigate)
    ) {
        let html = apply_pre_processes(
            context
                .http_client
                .page_navigate_for_request(&url, &headers)
                .await?,
            query.pre_processes(),
            query.name(),
            query.base_url(),
            context.params,
        )?;
        let fragment = fragment_from_response(validation.yaml_hash(), None, None, &html);
        let not_modified = incoming
            .content_hash
            .as_deref()
            .is_some_and(|expected| expected == hash62(html.as_bytes()));
        record_outcome(not_modified, fragment);
        return Ok(vec![(url, FetchedResponse::Html(html))]);
    }

    let response = context
        .http_client
        .send_for_request(method, &url, &headers, body.as_deref())
        .await?;
    let status = response.status();

    if query
        .http_config()
        .empty_on_statuses
        .contains(&status.as_u16())
    {
        let empty = empty_response(query.scraper_type());
        let fragment = fragment_from_response(
            validation.yaml_hash(),
            None,
            None,
            empty_response_body(query.scraper_type()),
        );
        record_outcome(false, fragment);
        return Ok(vec![(url, empty)]);
    }

    let etag_header = response
        .headers()
        .get("etag")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let last_modified_header = response
        .headers()
        .get("last-modified")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    // 304 Not Modified: reuse the server ETag when refreshed, otherwise the
    // incoming fragment (still valid — the content did not change).
    if status == http::StatusCode::NOT_MODIFIED && incoming.has_validator() {
        let fragment = match &etag_header {
            Some(etag) => format!(
                "{}{}-{}",
                crate::scrapyfy::ETAG_FRAGMENT_PREFIX,
                validation.yaml_hash(),
                urlencoding::encode(etag)
            ),
            None => validation
                .incoming_fragment()
                .unwrap_or_default()
                .to_string(),
        };
        record_outcome(true, fragment);
        return Ok(vec![(url, FetchedResponse::NotModified)]);
    }

    let text = apply_pre_processes(
        response.text().await?,
        query.pre_processes(),
        query.name(),
        query.base_url(),
        context.params,
    )?;
    let fragment = fragment_from_response(
        validation.yaml_hash(),
        etag_header.as_deref(),
        last_modified_header.as_deref(),
        &text,
    );

    // Content-hash staleness: the server ignored conditional headers but the
    // body is byte-identical to what the client already has.
    let not_modified = etag_header.is_none()
        && incoming
            .content_hash
            .as_deref()
            .is_some_and(|expected| expected == hash62(text.as_bytes()));

    record_outcome(not_modified, fragment);
    if not_modified {
        return Ok(vec![(url, FetchedResponse::NotModified)]);
    }

    let parsed = match query.scraper_type() {
        ScraperType::Json => {
            if query.extract_next_data() {
                FetchedResponse::Json(HttpClient::extract_next_data_json(&url, &text)?)
            } else {
                FetchedResponse::Json(serde_json::from_str(&text).map_err(|error| {
                    anyhow::anyhow!("Invalid JSON payload returned by {}: {error}", url)
                })?)
            }
        }
        ScraperType::Html => FetchedResponse::Html(text),
        ScraperType::Text => FetchedResponse::Text(text),
        ScraperType::Static => FetchedResponse::Static,
    };
    Ok(vec![(url, parsed)])
}

/// Resolves the request URL by formatting the template with resolved params.
///
/// The URL template is resolved using [`query_helpers::format_query_template`]
/// with the parameters from the context. `request_url_actions`, when present,
/// transform the resolved template before the HTTP call.
///
/// # Arguments
///
/// * `query` - The query containing the URL template.
/// * `context` - Runtime context containing parameters for template resolution.
/// * `response` - Optional parent response for sub-query URL resolution.
///
/// # Returns
///
/// A vector containing the resolved URL(s). Returns empty vector for sub-queries
/// without a query_url template when response/context is not provided.
pub(super) fn resolve_request_urls(
    query: &dyn ScraperQuery,
    context: &QueryContext<'_>,
) -> Result<Vec<String>> {
    // First, try to resolve from query_url template (root queries)
    if !query.query_url().is_empty() {
        let url = query_helpers::format_query_template(
            query.base_url(),
            query.query_url(),
            context.params,
        )
        .map_err(|error| {
            anyhow::anyhow!(
                "Failed to resolve request URL for query `{}`: {error}",
                query.name()
            )
        })?;
        let mut urls = vec![url];
        for action in query.request_url_actions() {
            urls = action.apply(&None, urls, context.params, context.request_url, None, None);
        }
        return Ok(urls);
    }

    if let Some(parent_response) = context.parent_response {
        if let Some(rp) = query.request_pointer() {
            if !rp.is_empty() {
                let values: Vec<String> =
                    select_json_values(parent_response, Some(rp), query.request_select())
                        .into_iter()
                        .flat_map(json_value_to_strings)
                        .collect();
                if !values.is_empty() {
                    let mut resolved_urls = Vec::new();
                    for value in values {
                        let mut url_values = vec![value];
                        for action in query.request_actions() {
                            url_values = action.apply(
                                &None,
                                url_values,
                                context.params,
                                context.request_url,
                                None,
                                None,
                            );
                        }
                        if let Some(url) = url_values.into_iter().find(|v| !v.is_empty()) {
                            resolved_urls.push(url);
                        }
                    }
                    return Ok(resolved_urls);
                }
            }
        } else if !query.request_actions().is_empty() {
            // No request_pointer but request_actions present — use the parent
            // response value directly as the input for the action pipeline.
            // This covers the PROMOBOX sub-chain pattern where
            // context_pointer: /mediaId selects a value and
            // request_actions: format_text builds the full URL.
            let values: Vec<String> =
                select_json_values(parent_response, None, query.request_select())
                    .into_iter()
                    .flat_map(json_value_to_strings)
                    .collect();
            if !values.is_empty() {
                let mut resolved_urls = Vec::new();
                for value in values {
                    let mut url_values = vec![value];
                    for action in query.request_actions() {
                        url_values = action.apply(
                            &None,
                            url_values,
                            context.params,
                            context.request_url,
                            None,
                            None,
                        );
                    }
                    for url in url_values {
                        if !url.is_empty() {
                            resolved_urls.push(url);
                        }
                    }
                }
                return Ok(resolved_urls);
            }
        }
    }
    Ok(vec![])
}

/// Resolves request headers from the query's configured header list.
///
/// Each header's value is resolved from the params context using the
/// header's pointer and actions pipeline.
///
/// # Arguments
///
/// * `headers` - The list of configured headers to resolve.
/// * `params` - Runtime template parameters.
/// * `request_url` - URL of the parent request for action resolution.
/// * `parent_response` - Optional JSON response used for pointer resolution.
///
/// # Returns
///
/// A hash map of header names to their resolved values.
pub(super) fn resolve_request_headers(
    headers: &[crate::scrapyfy::scraper_json::query::ScraperRequestHeader],
    params: &HashMap<String, String>,
    request_url: &str,
    parent_response: Option<&Value>,
) -> HashMap<String, String> {
    let request_context = query_helpers::build_params_json_value(params);
    let request_context = parent_response.unwrap_or(&request_context);
    let mut resolved = HashMap::new();
    for header in headers {
        if let Some((name, value)) = header.resolve(request_context, params, request_url) {
            resolved.insert(name, value);
        }
    }
    resolved
}

/// Resolves the optional request body from the query's body pointer/actions.
///
/// Selects JSON values from the parent response when available, otherwise from
/// the params context, then applies the body actions pipeline.
///
/// # Arguments
///
/// * `pointer` - Optional JSON pointer to select the body value.
/// * `select` - Selection mode for the pointer (First or All).
/// * `actions` - Actions to apply to the selected values.
/// * `params` - Runtime template parameters.
/// * `request_url` - URL of the parent request for action resolution.
/// * `parent_response` - Optional JSON response used for pointer resolution.
///
/// # Returns
///
/// The resolved request body as a string, or `None` if no body is configured
/// or if the resolved value is empty.
pub(super) fn resolve_request_body(
    pointer: Option<&str>,
    select: HtmlScraperSelectMode,
    actions: &[crate::scrapyfy::ScraperAction],
    params: &HashMap<String, String>,
    request_url: &str,
    parent_response: Option<&Value>,
) -> Option<String> {
    if pointer.is_none() && actions.is_empty() {
        return None;
    }
    let request_context = query_helpers::build_params_json_value(params);
    let request_context = parent_response.unwrap_or(&request_context);
    let mut values: Vec<String> = select_json_values(request_context, pointer, select)
        .into_iter()
        .flat_map(crate::scrapyfy::scraper_json::entry::json_value_to_strings)
        .collect();
    for action in actions {
        values = action.apply(&None, values, params, request_url, None, None);
    }
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

/// Fetches all responses in parallel using the configured HTTP client.
///
/// Executes HTTP requests for all URLs concurrently, using the configured
/// concurrency level ([`DEFAULT_FETCH_CONCURRENCY`]).
///
/// # Arguments
///
/// * `query` - The query being executed.
/// * `urls` - The URLs to fetch.
/// * `context` - Runtime context containing parameters and HTTP client.
///
/// # Returns
///
/// A vector of tuples containing the URL and its corresponding [`FetchedResponse`].
/// Results are ordered according to the input URLs.
///
/// # Errors
///
/// Returns an error if any HTTP request fails.
pub(super) async fn fetch_responses(
    query: &dyn ScraperQuery,
    urls: &[String],
    context: &QueryContext<'_>,
) -> Result<Vec<(String, FetchedResponse)>> {
    let method = query.request_method().as_http_method();
    let headers = resolve_request_headers(
        query.request_headers(),
        context.params,
        urls.first().map(String::as_str).unwrap_or(""),
        context.parent_response,
    );
    let body = resolve_request_body(
        query.request_body_pointer(),
        query.request_body_select(),
        query.request_body_actions(),
        context.params,
        urls.first().map(String::as_str).unwrap_or(""),
        context.parent_response,
    );
    // Conditional validation applies to the root request of a source only;
    // sub-query contexts always carry a None validation slot.
    if let Some(validation) = context.validation {
        return fetch_responses_with_validation(query, urls, context, validation).await;
    }

    let extract_next_data = query.extract_next_data();
    let scraper_type = query.scraper_type();
    let execution = query.http_config().execution;
    match execution {
        Some(ScraperHttpExecution::PageNavigate) => {
            if scraper_type != ScraperType::Html {
                anyhow::bail!("page_navigate is only supported by HTML queries");
            }
            if query.http_config().browser_context != Some(ScraperBrowserContext::Origin) {
                anyhow::bail!("page_navigate requires http.browser_context: origin");
            }
            if method != http::Method::GET {
                anyhow::bail!("page_navigate only supports GET queries");
            }
        }
        Some(ScraperHttpExecution::PageFetch) => {
            anyhow::bail!("page_fetch is only supported by HTML sub-queries");
        }
        Some(ScraperHttpExecution::PageClick) => {
            anyhow::bail!("page_click is only supported by HTML sub-queries");
        }
        Some(ScraperHttpExecution::Direct) | None => {}
    }
    // Filter out empty URLs before fetching.
    let non_empty_urls: Vec<String> = urls
        .iter()
        .filter(|url| !url.trim().is_empty())
        .cloned()
        .collect();

    let jobs = non_empty_urls
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, url)| {
            let headers = headers.clone();
            let body = body.clone();
            let method = method.clone();
            let client = context.http_client;
            async move {
                let response = if execution == Some(ScraperHttpExecution::PageNavigate) {
                    FetchedResponse::Html(apply_pre_processes(
                        client.page_navigate_for_request(&url, &headers).await?,
                        query.pre_processes(),
                        query.name(),
                        query.base_url(),
                        context.params,
                    )?)
                } else {
                    fetch_single(
                        &client,
                        method,
                        &url,
                        &headers,
                        body.as_deref(),
                        extract_next_data,
                        scraper_type,
                        &query.http_config().empty_on_statuses,
                        query.pre_processes(),
                        query.name(),
                        query.base_url(),
                        context.params,
                    )
                    .await?
                };
                Ok::<(usize, (String, FetchedResponse)), anyhow::Error>((index, (url, response)))
            }
        });

    let results: Vec<Result<(usize, (String, FetchedResponse))>> = stream::iter(jobs)
        .buffer_unordered(DEFAULT_FETCH_CONCURRENCY)
        .collect()
        .await;

    let mut ordered = Vec::with_capacity(results.len());
    for result in results {
        ordered.push(result?);
    }
    ordered.sort_by_key(|(index, _)| *index);
    Ok(ordered.into_iter().map(|(_, value)| value).collect())
}
