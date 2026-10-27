//! Origin normalization and redirect URL/method decisions.

use super::*;

/// Normalizes a URL or origin string to an origin URL.
///
/// This function takes a URL or origin string and normalizes it to a consistent
/// origin format (scheme://host:port/). It's used throughout the client to ensure
/// consistent cookie and Cloudflare state management.
///
/// # Parameters
///
/// - `value`: Absolute URL or origin string to normalize
///
/// # Returns
///
/// A normalized origin URL string in the format `scheme://host:port/`
///
/// # Errors
///
/// Returns `InvalidUrl` when parsing fails or the URL has no host
///
/// # Examples
///
/// ```
/// # use arachnea_http::error::ArachneaHttpError;
/// # fn example() -> Result<(), ArachneaHttpError> {
/// # use url::Url;
/// let origin = Url::parse("https://example.com/path?query=value")
///     .map(|url| {
///         let scheme = url.scheme();
///         let host = url.host_str().unwrap();
///         let port = url.port().map(|port| format!(":{port}")).unwrap_or_default();
///         format!("{scheme}://{host}{port}/")
///     })
///     .expect("valid URL");
/// assert_eq!(origin, "https://example.com/");
/// # Ok(())
/// # }
/// ```
pub(super) fn origin_url(value: &str) -> Result<String, ArachneaHttpError> {
    let url = Url::parse(value).map_err(|err| ArachneaHttpError::InvalidUrl(err.to_string()))?;
    let scheme = url.scheme();
    let host = url
        .host_str()
        .ok_or_else(|| ArachneaHttpError::InvalidUrl("URL must contain a host".to_string()))?;
    let port = url
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    Ok(format!("{scheme}://{host}{port}/"))
}

/// Extracts the next redirect target URL from a response, if one exists.
///
/// Returns None when the response is not a redirection or has no Location
/// header. Resolves relative locations against the response URL.
///
/// # Parameters
///
/// - Response: Materialized response to inspect.
///
/// # Returns
///
/// An absolute redirect target URL, or None.
///
/// # Errors
///
/// Returns InvalidHeader or InvalidUrl when the Location header or its
/// resolution fails.
pub(super) fn redirect_target(
    response: &ArachneaResponse,
) -> Result<Option<String>, ArachneaHttpError> {
    if !response.status.is_redirection() {
        return Ok(None);
    }

    let Some(location) = response.headers.get(LOCATION) else {
        return Ok(None);
    };
    let location = location
        .to_str()
        .map_err(|err| ArachneaHttpError::InvalidHeader(err.to_string()))?;
    let base =
        Url::parse(&response.url).map_err(|err| ArachneaHttpError::InvalidUrl(err.to_string()))?;
    let next = base
        .join(location)
        .map_err(|err| ArachneaHttpError::InvalidUrl(err.to_string()))?;
    Ok(Some(next.to_string()))
}

/// Determines whether a redirect status code should rewrite the method to GET.
///
/// POST, PUT, and PATCH are rewritten to GET for 303 See Other. 302 Found and
/// 301 Moved Permanently are also rewritten except when the original method is
/// GET or HEAD.
///
/// # Parameters
///
/// - status: HTTP redirect status code.
/// - method: Original request method.
///
/// # Returns
///
/// `true` when the method should be rewritten to GET for the next redirect hop.
pub(super) fn should_rewrite_redirect_to_get(status: StatusCode, method: &Method) -> bool {
    if *method == Method::GET || *method == Method::HEAD {
        return false;
    }

    status == StatusCode::SEE_OTHER
        || status == StatusCode::FOUND
        || status == StatusCode::MOVED_PERMANENTLY
}

#[cfg(test)]
#[path = "../../tests/client/urls.rs"]
mod tests;
