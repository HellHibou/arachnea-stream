/// Rquest integration mode.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RquestIntegrationMode {
    /// Use a loopback proxy URL configured through `rquest::Proxy`.
    LoopbackProxy,
}

/// Notes the current rquest support status.
pub fn support_status() -> &'static str {
    "rquest in-process transport replacement is not stable here; use a loopback proxy helper"
}

#[cfg(feature = "rquest")]
use std::sync::Arc;

#[cfg(feature = "rquest")]
use crate::core::{
    context_from_parameter_pairs, ArachneaProxyCore, ClientContext, ParameterDefinition,
};
#[cfg(feature = "rquest")]
use crate::server::{LoopbackProxyRuntime, LoopbackRoute};
#[cfg(feature = "rquest")]
use rquest::header::{HeaderMap, HeaderName, HeaderValue};

#[cfg(feature = "rquest")]
use crate::connectors::{ConnectorError, Result};

/// Loopback proxy helper for rquest clients.
#[cfg(feature = "rquest")]
pub struct ArachneaRquestLoopback {
    proxy_url: String,
    parameter_definitions: Vec<ParameterDefinition>,
    runtime: Arc<LoopbackProxyRuntime>,
    _route: Arc<LoopbackRoute>,
}

#[cfg(feature = "rquest")]
impl ArachneaRquestLoopback {
    /// Starts a loopback HTTP proxy listener backed by the supplied core.
    ///
    /// # Arguments
    ///
    /// - `core`: Proxy core used by the loopback HTTP listener.
    ///
    /// # Returns
    ///
    /// Loopback helper retaining a routing lease on the shared listener.
    ///
    /// # Errors
    ///
    /// Returns an error when the loopback listener cannot bind.
    pub async fn start(core: ArachneaProxyCore) -> Result<Self> {
        let parameter_definitions = core.parameter_definitions();
        Self::start_with_client_context(core, parameter_definitions, ClientContext::new()).await
    }

    /// Starts a loopback helper with fixed routing parameters for its connections.
    ///
    /// This is intended for clients such as Chrome that cannot attach custom
    /// headers to HTTP proxy CONNECT requests.
    ///
    /// # Arguments
    /// * `core` - Proxy core used to route and establish outbound connections.
    /// * `parameters` - Per-client proxy parameters converted into routing context.
    ///
    /// # Errors
    ///
    /// Returns an error if route registration or binding the loopback listener fails.
    pub async fn start_with_parameters<'a>(
        core: ArachneaProxyCore,
        parameters: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Self> {
        let parameter_definitions = core.parameter_definitions();
        let context = context_from_parameter_pairs(parameters, &parameter_definitions);
        Self::start_with_client_context(core, parameter_definitions, context).await
    }

    /// Starts a loopback listener with one client context applied to every connection.
    ///
    /// # Arguments
    /// * `core` - Proxy core used to route and establish outbound connections.
    /// * `parameter_definitions` - Definitions mapping client parameters to their HTTP headers.
    /// * `client_context` - Routing parameters attached to requests handled by this client route.
    ///
    async fn start_with_client_context(
        core: ArachneaProxyCore,
        parameter_definitions: Vec<ParameterDefinition>,
        client_context: ClientContext,
    ) -> Result<Self> {
        Self::start_on_runtime(
            Arc::new(LoopbackProxyRuntime::default()),
            core,
            parameter_definitions,
            client_context,
            true,
        )
        .await
    }

    /// Registers a routing lease on an existing shared listener.
    ///
    /// # Arguments
    /// - `runtime`: Application-owned listener shared by all derived clients.
    /// - `core`: General or request-pinned routing core.
    /// - `parameter_definitions`: Accepted parameter headers.
    /// - `client_context`: Connection-local default routing parameters.
    /// - `allow_parameter_headers`: Whether the lease accepts additional parameter headers.
    ///
    /// # Errors
    /// Returns an error if binding or routing-context registration fails.
    pub async fn start_on_runtime(
        runtime: Arc<LoopbackProxyRuntime>,
        core: ArachneaProxyCore,
        parameter_definitions: Vec<ParameterDefinition>,
        client_context: ClientContext,
        allow_parameter_headers: bool,
    ) -> Result<Self> {
        let addr = runtime.start().await?;
        let route = runtime.register(core, client_context, allow_parameter_headers)?;
        Ok(Self {
            proxy_url: format!("http://arachnea-context:{}@{addr}", route.credential()),
            parameter_definitions,
            runtime,
            _route: route,
        })
    }

    /// Returns the shared listener used by this routing lease.
    pub fn runtime(&self) -> Arc<LoopbackProxyRuntime> {
        self.runtime.clone()
    }

    /// Rebinds an idle, exclusively owned transport to the current request's core.
    /// Callers must validate route compatibility before reusing its HTTP client.
    ///
    /// # Arguments
    /// - `core`: Fresh selection carrying the current request's token and deadline.
    ///
    /// # Errors
    /// Returns an error if the internal routing lease can no longer be rebound.
    pub fn rebind(&mut self, core: ArachneaProxyCore) -> Result<()> {
        self._route = self.runtime.rebind_internal_route(&self._route, core)?;
        Ok(())
    }

    /// Returns the proxy URL usable with `rquest::Proxy`.
    ///
    /// # Returns
    ///
    /// Loopback proxy URL selected when the helper started.
    pub fn proxy_url(&self) -> &str {
        &self.proxy_url
    }

    /// Returns parameter definitions captured from the proxy core.
    ///
    /// # Returns
    ///
    /// Parameter definitions accepted by the loopback proxy.
    pub fn parameter_definitions(&self) -> &[ParameterDefinition] {
        &self.parameter_definitions
    }

    /// Builds a rquest client configured to use the loopback proxy.
    ///
    /// # Returns
    ///
    /// Rquest client configured for this loopback proxy.
    ///
    /// # Errors
    ///
    /// Returns an error when the proxy cannot be configured or the rquest
    /// client cannot be built.
    pub fn client(&self) -> Result<rquest::Client> {
        self.client_with_connect_timeout(None)
    }

    /// Builds a loopback client with an explicit connection-establishment limit.
    ///
    /// # Arguments
    /// - `timeout`: Complete connector budget, or `None` for the ordinary default.
    ///
    /// # Errors
    /// Returns an error when the proxy or client cannot be configured.
    pub fn client_with_connect_timeout(
        &self,
        timeout: Option<std::time::Duration>,
    ) -> Result<rquest::Client> {
        let proxy = rquest::Proxy::all(&self.proxy_url)
            .map_err(|error| ConnectorError::Rquest(error.to_string()))?;
        let mut builder = rquest::Client::builder()
            .proxy(proxy)
            .redirect(rquest::redirect::Policy::none());
        if let Some(timeout) = timeout {
            builder = builder.connect_timeout(timeout);
        }
        builder
            .build()
            .map_err(|error| ConnectorError::Rquest(error.to_string()))
    }

    /// Builds a rquest client that sends routing parameters to the loopback proxy.
    ///
    /// Parameters are encoded as proxy-level HTTP headers so they are visible
    /// on both absolute-form HTTP requests and HTTPS `CONNECT` requests. The
    /// headers are selected from the core parameter definitions captured when
    /// the loopback listener started.
    ///
    /// # Arguments
    ///
    /// - `parameters`: Request-parameter pairs encoded as `name=value`.
    ///
    /// # Returns
    ///
    /// Rquest client configured for this loopback proxy.
    ///
    /// # Errors
    ///
    /// Returns an error when a parameter is unknown, a header is invalid, the
    /// proxy cannot be configured, or the rquest client cannot be built.
    pub fn client_with_parameters<'a>(
        &self,
        parameters: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<rquest::Client> {
        let mut headers = HeaderMap::new();
        for (name, value) in parameters {
            let header = self
                .parameter_definitions
                .iter()
                .find(|definition| {
                    definition.name == name || definition.http_header.eq_ignore_ascii_case(name)
                })
                .map(|definition| definition.http_header.as_str())
                .unwrap_or(name);
            let header_name = HeaderName::from_bytes(header.as_bytes()).map_err(|error| {
                ConnectorError::Rquest(format!(
                    "invalid proxy parameter header '{}': {error}",
                    header
                ))
            })?;

            let header_value = HeaderValue::from_str(value).map_err(|error| {
                ConnectorError::Rquest(format!(
                    "invalid proxy parameter value for '{}': {error}",
                    name
                ))
            })?;
            headers.insert(header_name, header_value);
        }

        let proxy = rquest::Proxy::all(&self.proxy_url)
            .map_err(|error| ConnectorError::Rquest(error.to_string()))?
            .custom_http_headers(headers);
        rquest::Client::builder()
            .proxy(proxy)
            .redirect(rquest::redirect::Policy::none())
            .build()
            .map_err(|error| ConnectorError::Rquest(error.to_string()))
    }
}
