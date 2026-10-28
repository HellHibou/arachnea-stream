//! Materialized HTTP responses and body decoding.

use super::*;

/// Materialized HTTP response returned by Arachnea.
#[derive(Debug, Clone)]
pub struct ArachneaResponse {
    /// Decision observed before body materialization or transformations.
    #[cfg(feature = "arachnea-proxy")]
    pub(super) proxy_response_decision:
        Option<arachnea_proxy::core::http::response_classifier::ProxyResponseDecision>,
    /// Exact dynamic route used for this response, absent for uncorrelated engines.
    #[cfg(feature = "arachnea-proxy")]
    pub(super) proxy_selection: Option<arachnea_proxy::core::ProxySelectionToken>,
    /// Final URL reported by the engine.
    pub(super) url: String,
    /// Redirect target URLs followed by the HTTP facade.
    pub(super) redirect_urls: Vec<String>,
    /// HTTP status code.
    pub(super) status: StatusCode,
    /// Response headers.
    pub(super) headers: HeaderMap,
    /// Response body bytes.
    pub(super) body: Bytes,
}

impl ArachneaResponse {
    /// Returns the early decision, absent for engines without observation support.
    #[cfg(feature = "arachnea-proxy")]
    pub fn proxy_response_decision(
        &self,
    ) -> Option<&arachnea_proxy::core::http::response_classifier::ProxyResponseDecision> {
        self.proxy_response_decision.as_ref()
    }
    /// Converts an engine response into a public response.
    ///
    /// # Arguments
    ///
    /// - `response`: Engine-normalized response.
    ///
    /// # Returns
    ///
    /// A public response wrapper.
    pub(super) fn from_engine(response: crate::engine::EngineResponse) -> Self {
        Self {
            #[cfg(feature = "arachnea-proxy")]
            proxy_response_decision: None,
            #[cfg(feature = "arachnea-proxy")]
            proxy_selection: None,
            url: response.url,
            redirect_urls: Vec::new(),
            status: response.status,
            headers: response.headers,
            body: response.body,
        }
    }

    /// Builds an in-memory successful text response.
    ///
    /// The response has status `200 OK`, empty headers, and no redirect
    /// history. This is intended for local fixtures and caller-side mock routers
    /// that need to share the same response helpers as real HTTP responses.
    ///
    /// # Arguments
    ///
    /// - `url`: Final response URL.
    /// - `body`: Complete response body.
    ///
    /// # Returns
    ///
    /// A materialized successful response.
    pub fn from_text(url: impl Into<String>, body: impl Into<Bytes>) -> Self {
        Self {
            #[cfg(feature = "arachnea-proxy")]
            proxy_response_decision: None,
            #[cfg(feature = "arachnea-proxy")]
            proxy_selection: None,
            url: url.into(),
            redirect_urls: Vec::new(),
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: body.into(),
        }
    }

    /// Returns the final URL.
    ///
    /// # Returns
    ///
    /// Final response URL as a string slice.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns the request-owned selection, never inferred from a destination map.
    #[cfg(feature = "arachnea-proxy")]
    pub fn proxy_selection(&self) -> Option<&arachnea_proxy::core::ProxySelectionToken> {
        self.proxy_selection.as_ref()
    }

    /// Returns redirect target URLs followed before this response.
    ///
    /// # Returns
    ///
    /// Redirect target URLs in request order.
    pub fn redirect_urls(&self) -> &[String] {
        &self.redirect_urls
    }

    /// Replaces the redirect target URL history.
    ///
    /// # Arguments
    ///
    /// - `redirect_urls`: Redirect targets in the order they were followed.
    pub(super) fn set_redirect_urls(&mut self, redirect_urls: Vec<String>) {
        self.redirect_urls = redirect_urls;
    }

    /// Returns the HTTP status.
    ///
    /// # Returns
    ///
    /// Response status code.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Returns response headers.
    ///
    /// # Returns
    ///
    /// Borrowed response header map.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Returns response body bytes.
    ///
    /// # Returns
    ///
    /// Cloned body bytes.
    pub async fn bytes(&self) -> Result<Bytes, ArachneaHttpError> {
        Ok(self.body.clone())
    }

    /// Decodes the response body as text using the charset declared in `Content-Type`.
    ///
    /// When the `Content-Type` header specifies a known charset (e.g.
    /// `text/html; charset=iso-8859-1`), the body is decoded with that encoding
    /// via `encoding_rs`. Otherwise the body is tried as strict UTF-8 first; if
    /// that fails, it is decoded as `windows-1252` (the superset of ISO-8859-1
    /// commonly used for legacy HTML pages). Both fallbacks use replacement
    /// characters for invalid sequences.
    ///
    /// # Returns
    ///
    /// The response body as a `String`.
    ///
    /// # Errors
    ///
    /// Always succeeds — the body is always decodable by at least one fallback strategy.
    pub async fn text(&self) -> Result<String, ArachneaHttpError> {
        // 1. Try the charset declared in Content-Type (e.g. iso-8859-1, utf-8)
        let encoding = self
            .headers
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|ct| charset_from_content_type(ct))
            .and_then(|label| Encoding::for_label(label.as_bytes()));

        let output = if let Some(encoding) = encoding {
            let (decoded, _, _) = encoding.decode(&self.body);
            decoded.to_string()
        } else {
            // 2. Try strict UTF-8 first (common case)
            let bytes = &self.body;
            if let Ok(s) = std::str::from_utf8(bytes) {
                s.to_string()
            } else {
                // 3. Fallback to windows-1252 (ISO-8859-1 superset, many legacy sites)
                let (decoded, _, _) = WINDOWS_1252.decode(bytes);
                decoded.to_string()
            }
        };

        Ok(output)
    }

    /// Parses the response body as JSON.
    ///
    /// # Returns
    ///
    /// The deserialized JSON payload.
    ///
    /// # Errors
    ///
    /// Returns `Json` when the response body cannot be parsed.
    pub fn json<T>(&self) -> Result<T, ArachneaHttpError>
    where
        T: DeserializeOwned,
    {
        serde_json::from_slice(&self.body).map_err(|err| ArachneaHttpError::Json(err.to_string()))
    }

    /// Returns a bounded preview of the body for detection.
    ///
    /// # Returns
    ///
    /// At most the first 4096 body bytes.
    pub(super) fn preview_body(&self) -> &[u8] {
        let end = self.body.len().min(4096);
        &self.body[..end]
    }
}
