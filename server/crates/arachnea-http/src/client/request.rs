//! Outbound request builder and redirect replay options.

use super::*;

/// Builder for one outbound request.
pub struct ArachneaRequestBuilder {
    /// Owning client facade.
    pub(super) client: ArachneaHttpClient,
    /// HTTP method.
    pub(super) method: Method,
    /// Absolute URL.
    pub(super) url: String,
    /// Optional per-request execution mode.
    pub(super) mode: Option<HttpRequestMode>,
    /// Request headers.
    pub(super) headers: HeaderMap,
    /// Optional request body.
    pub(super) body: Option<Bytes>,
}

impl ArachneaRequestBuilder {
    /// Adds or replaces one header.
    ///
    /// # Parameters
    ///
    /// - `name`: Header name.
    /// - `value`: Header value.
    ///
    /// # Returns
    ///
    /// The updated request builder.
    pub fn header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.insert(name, value);
        self
    }

    /// Adds or replaces multiple headers.
    ///
    /// # Parameters
    ///
    /// - `headers`: Header map merged into this request.
    ///
    /// # Returns
    ///
    /// The updated request builder.
    pub fn headers(mut self, headers: HeaderMap) -> Self {
        self.headers.extend(headers);
        self
    }

    /// Sets the execution mode for this request.
    ///
    /// # Parameters
    ///
    /// - `mode`: Predefined request mode.
    ///
    /// # Returns
    ///
    /// The updated request builder.
    pub fn mode(mut self, mode: HttpRequestMode) -> Self {
        self.mode = Some(mode);
        self
    }

    /// Sets a raw request body.
    ///
    /// # Parameters
    ///
    /// - `body`: Body bytes.
    ///
    /// # Returns
    ///
    /// The updated request builder.
    pub fn body(mut self, body: impl Into<Bytes>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Serializes and sets a JSON request body.
    ///
    /// # Parameters
    ///
    /// - `value`: Serializable JSON value.
    ///
    /// # Returns
    ///
    /// The updated request builder or a JSON serialization error.
    pub fn json<T: Serialize>(mut self, value: &T) -> Result<Self, ArachneaHttpError> {
        let body =
            serde_json::to_vec(value).map_err(|err| ArachneaHttpError::Json(err.to_string()))?;
        self.headers
            .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        self.body = Some(Bytes::from(body));
        Ok(self)
    }

    /// Sends the prepared request.
    ///
    /// # Returns
    ///
    /// A materialized HTTP response.
    ///
    /// # Errors
    ///
    /// Returns URL, engine, cookie, JSON, or Cloudflare errors.
    pub async fn send(self) -> Result<ArachneaResponse, ArachneaHttpError> {
        let options = RequestOptions {
            method: self.method,
            url: self.url,
            mode: self.mode,
            headers: self.headers,
            body: self.body,
        };
        Box::pin(self.client.execute(options)).await
    }
}

/// Prepared request options used internally by the client.
#[derive(Debug, Clone)]
pub struct RequestOptions {
    /// HTTP method.
    pub method: Method,
    /// Absolute request URL.
    pub url: String,
    /// Optional per-request execution mode.
    pub mode: Option<HttpRequestMode>,
    /// Request headers.
    pub headers: HeaderMap,
    /// Optional request body.
    pub body: Option<Bytes>,
}

impl RequestOptions {
    /// Builds the next request options in a redirect chain.
    pub(super) fn for_redirect(mut self, url: String, status: StatusCode) -> Self {
        if should_rewrite_redirect_to_get(status, &self.method) {
            self.method = Method::GET;
            self.body = None;
            self.headers.remove(CONTENT_TYPE);
        }
        self.url = url;
        self
    }
}
