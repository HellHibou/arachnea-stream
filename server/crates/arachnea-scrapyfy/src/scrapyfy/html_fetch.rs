//! HTML document retrieval with explicit literal JavaScript redirect following.

use std::collections::HashMap;

use anyhow::{bail, Context, Result};

use super::HttpClient;

mod redirects;
use redirects::{extract_simple_js_redirect_target, resolve_redirect_target};

/// Document returned after the configured HTTP and literal JavaScript redirects.
pub struct FetchedHtmlDocument {
    /// Final URL reported by the HTTP client for the accepted document.
    pub url: String,
    /// Decoded HTML body of the accepted document.
    pub html: String,
}

/// Explicit document acceptance and literal JavaScript redirect policy.
///
/// HTTP redirect limits belong to the supplied client's `ScraperHttpConfig` and
/// apply independently to each fetch. This policy does not bound network traffic
/// or allocations: the HTTP response body is already materialized before checking.
pub struct HtmlFetchPolicy {
    /// Maximum accepted body size in bytes, checked after receipt and before decoding.
    pub max_html_bytes: usize,
    /// Optional maximum followed JavaScript hops. `None` disables recognition;
    /// `Some(0)` recognizes redirects but rejects any hop requiring another fetch.
    pub max_js_redirects: Option<usize>,
}

/// Fetches a successful `text/html` document with optional literal redirects.
///
/// Recognition scans text for the first simple `window.location.href` literal
/// assignment; it does not parse or execute JavaScript. Relative targets use the
/// response's final URL, and only HTTP(S) targets are followed. An assignment to
/// that same final URL is ignored. Cycles use exact requested-URL strings, not
/// all HTTP intermediate/final URLs, and share the hop-limit error diagnostic.
///
/// # Arguments
/// * `client` - Scrapyfy client configured with its independent HTTP redirect budget.
/// * `url` - Initial document URL, validated by the supplied HTTP client.
/// * `policy` - Accepted body size and explicit JavaScript redirect policy.
///
/// # Returns
/// The final URL and decoded HTML, using the HTTP response's existing text decoder.
///
/// # Errors
/// Returns HTTP/reading errors, unsuccessful status, non-HTML content type,
/// oversized materialized body, repeated requested URL or excessive JavaScript hops.
pub async fn fetch_html_document(
    client: &HttpClient,
    url: &str,
    policy: &HtmlFetchPolicy,
) -> Result<FetchedHtmlDocument> {
    let mut current_url = url.to_string();
    let mut visited: Vec<String> = Vec::new();

    for _ in 0..=policy.max_js_redirects.unwrap_or(0) {
        if visited.iter().any(|value| value == &current_url) {
            break;
        }
        visited.push(current_url.clone());

        let response = client
            .send_for_request(http::Method::GET, &current_url, &HashMap::new(), None)
            .await
            .with_context(|| format!("Failed to fetch HTML document `{}`", current_url))?;
        let response_url = response.url().to_string();

        if !response.status().is_success() {
            bail!(
                "Failed to fetch HTML document `{}`: HTTP status {}",
                current_url,
                response.status()
            );
        }

        let content_type = response
            .headers()
            .get(http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        let mime_type = content_type
            .split(';')
            .next()
            .map(str::trim)
            .unwrap_or_default();

        if !mime_type.eq_ignore_ascii_case("text/html") {
            bail!(
                "Failed to inspect HTML document `{}`: unsupported Content-Type `{}`",
                current_url,
                content_type
            );
        }

        let bytes = response.bytes().await?;
        if bytes.len() > policy.max_html_bytes {
            bail!(
                "Failed to inspect HTML document `{}`: body is {} bytes, limit is {} bytes",
                current_url,
                bytes.len(),
                policy.max_html_bytes
            );
        }

        let body = response
            .text()
            .await
            .with_context(|| format!("Failed to read HTML document `{}`", current_url))?;

        if let Some(target) = policy
            .max_js_redirects
            .and_then(|_| extract_simple_js_redirect_target(&body))
        {
            if let Some(resolved) = resolve_redirect_target(&response_url, &target) {
                if resolved != response_url {
                    tracing::debug!(target: "arachnea_scrapyfy::scrapyfy::html_fetch",
                        current = %response_url,
                        redirect = %resolved,
                        "Following simple JS window.location.href redirect while fetching HTML document"
                    );
                    current_url = resolved;
                    continue;
                }
            }
        }

        return Ok(FetchedHtmlDocument {
            url: response_url,
            html: body,
        });
    }

    bail!(
        "Failed to fetch HTML document `{}`: too many JS redirects",
        url
    );
}
