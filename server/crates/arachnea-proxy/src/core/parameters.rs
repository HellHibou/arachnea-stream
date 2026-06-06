use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::core::{ClientContext, ClientParameter, ProxyError, ProxyNode, Result, TransportKind};

/// Proxy country parameter header name.
pub const PROXY_HEADER_PARAMETER_COUNTRY: &str = "Arachnea-Proxy-Country";

/// Proxy country parameter name.
pub const PROXY_PARAMETER_COUNTRY: &str = "country";

/// One inbound parameter definition shared by servers and routing handlers.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParameterDefinition {
    /// HTTP header name that carries the parameter on HTTP proxy requests.
    pub http_header: String,
    /// Context parameter name used by routing code.
    pub name: String,
    /// Whether the inbound HTTP header should be forwarded to the destination.
    pub forward_header: bool,
}

impl ParameterDefinition {
    /// Creates a parameter definition.
    ///
    /// # Parameters
    ///
    /// - `http_header`: HTTP header name that carries the parameter.
    /// - `name`: Context parameter name used by routing code.
    /// - `forward_header`: Whether the HTTP header is forwarded upstream.
    ///
    /// # Returns
    ///
    /// Parameter definition ready to register.
    pub fn new(
        http_header: impl Into<String>,
        name: impl Into<String>,
        forward_header: bool,
    ) -> Self {
        Self {
            http_header: http_header.into(),
            name: name.into(),
            forward_header,
        }
    }
}

/// Result returned by a parameter handler.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParameterHandlerDecision {
    /// Request-local extra proxy appended after the selected chain.
    pub extra_proxy: Option<ProxyNode>,
    /// Whether subsequent parameter handlers should be skipped.
    pub stop: bool,
}

impl ParameterHandlerDecision {
    /// Creates a decision that does not modify the selected chain.
    ///
    /// # Returns
    ///
    /// Decision that continues with the next handler.
    pub fn continue_without_proxy() -> Self {
        Self {
            extra_proxy: None,
            stop: false,
        }
    }

    /// Creates a decision that appends an extra proxy.
    ///
    /// # Parameters
    ///
    /// - `proxy`: Extra request-local proxy node.
    /// - `stop`: Whether following handlers should be skipped.
    ///
    /// # Returns
    ///
    /// Decision carrying the extra proxy node.
    pub fn with_proxy(proxy: ProxyNode, stop: bool) -> Self {
        Self {
            extra_proxy: Some(proxy),
            stop,
        }
    }
}

/// Trait implemented by request-parameter routing extensions.
pub trait ProxyParameterHandler: Send + Sync + std::fmt::Debug {
    /// Returns the inbound parameters understood by this handler.
    ///
    /// # Returns
    ///
    /// Parameter definitions used to build the shared extraction registry.
    fn parameter_definitions(&self) -> Vec<ParameterDefinition>;

    /// Selects a request-local extra proxy from client parameters.
    ///
    /// # Parameters
    ///
    /// - `parameters`: Client context built from inbound proxy parameters.
    ///
    /// # Returns
    ///
    /// Handler decision describing the extra proxy, if any.
    ///
    /// # Errors
    ///
    /// Returns an error when handler-specific validation fails.
    fn proxy_from_parameters(&self, parameters: &ClientContext)
        -> Result<ParameterHandlerDecision>;
}

/// Serializable parameter handler kind.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParameterHandlerKind {
    /// Handler that consumes Smart DNS route hints.
    SmartDns,
    /// Handler that maps a country parameter to an egress proxy.
    CountryRouting,
}

/// One configured parameter value to proxy mapping.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParameterProxyRoute {
    /// Parameter value that activates the route.
    pub value: String,
    /// Request-local proxy appended when `value` matches.
    pub proxy: ProxyNode,
}

/// Serializable configuration for built-in parameter handlers.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParameterHandlerConfig {
    /// Built-in handler implementation to instantiate.
    pub kind: ParameterHandlerKind,
    /// Context parameter name used by this handler.
    #[serde(default)]
    pub parameter_name: Option<String>,
    /// HTTP header used to receive this parameter.
    #[serde(default)]
    pub http_header: Option<String>,
    /// Whether the parameter HTTP header is forwarded upstream.
    #[serde(default)]
    pub forward_header: bool,
    /// Whether matching this handler stops subsequent handlers.
    #[serde(default = "default_true")]
    pub stop_on_match: bool,
    /// Parameter value to extra proxy mappings.
    #[serde(default)]
    pub routes: Vec<ParameterProxyRoute>,
}

/// Handler that maps Smart DNS hints to request-local proxy nodes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SmartDnsProxyHandler {
    definition: ParameterDefinition,
    routes: BTreeMap<String, ProxyNode>,
    stop_on_match: bool,
}

impl SmartDnsProxyHandler {
    /// Creates an empty Smart DNS handler.
    ///
    /// # Returns
    ///
    /// Handler using `Arachnea-Proxy-Smart-Dns` and `smart_dns`.
    pub fn new() -> Self {
        Self {
            definition: ParameterDefinition::new("Arachnea-Proxy-Smart-Dns", "smart_dns", false),
            routes: BTreeMap::new(),
            stop_on_match: true,
        }
    }

    /// Builds a Smart DNS handler from configuration.
    ///
    /// # Parameters
    ///
    /// - `config`: Serializable handler configuration.
    ///
    /// # Returns
    ///
    /// Configured Smart DNS handler.
    ///
    /// # Errors
    ///
    /// Returns an error when one configured route uses a direct node.
    pub fn from_config(config: &ParameterHandlerConfig) -> Result<Self> {
        let mut handler = Self::new();
        if let Some(parameter_name) = &config.parameter_name {
            handler.definition.name = parameter_name.clone();
        }
        if let Some(http_header) = &config.http_header {
            handler.definition.http_header = http_header.clone();
        }
        handler.definition.forward_header = config.forward_header;
        handler.stop_on_match = config.stop_on_match;
        for route in &config.routes {
            handler = handler.with_proxy_for_hint(&route.value, route.proxy.clone())?;
        }
        Ok(handler)
    }

    /// Adds a Smart DNS hint to proxy mapping.
    ///
    /// # Parameters
    ///
    /// - `hint`: Smart DNS hint value.
    /// - `proxy`: Extra proxy appended when `hint` matches.
    ///
    /// # Returns
    ///
    /// Updated handler.
    ///
    /// # Errors
    ///
    /// Returns an error when `proxy` is a direct node.
    pub fn with_proxy_for_hint(mut self, hint: impl AsRef<str>, proxy: ProxyNode) -> Result<Self> {
        validate_extra_proxy(&proxy)?;
        self.routes.insert(hint.as_ref().trim().to_string(), proxy);
        Ok(self)
    }
}

impl Default for SmartDnsProxyHandler {
    /// Returns the default Smart DNS parameter handler.
    fn default() -> Self {
        Self::new()
    }
}

impl ProxyParameterHandler for SmartDnsProxyHandler {
    /// Returns the Smart DNS parameter definition.
    fn parameter_definitions(&self) -> Vec<ParameterDefinition> {
        vec![self.definition.clone()]
    }

    /// Appends the proxy mapped to the Smart DNS hint when present.
    ///
    /// # Parameters
    ///
    /// - `parameters`: Client context containing optional Smart DNS hints.
    ///
    /// # Returns
    ///
    /// Handler decision with an extra proxy when a configured hint matches.
    ///
    /// # Errors
    ///
    /// This implementation does not currently fail.
    fn proxy_from_parameters(
        &self,
        parameters: &ClientContext,
    ) -> Result<ParameterHandlerDecision> {
        let Some(value) = parameters.get_string(&self.definition.name) else {
            return Ok(ParameterHandlerDecision::continue_without_proxy());
        };
        Ok(self
            .routes
            .get(value)
            .cloned()
            .map_or_else(ParameterHandlerDecision::continue_without_proxy, |proxy| {
                ParameterHandlerDecision::with_proxy(proxy, self.stop_on_match)
            }))
    }
}

/// Handler that maps countries to request-local proxy nodes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CountryRoutingProxyHandler {
    definition: ParameterDefinition,
    routes: BTreeMap<String, ProxyNode>,
    stop_on_match: bool,
}

impl CountryRoutingProxyHandler {
    /// Creates an empty country-routing handler.
    ///
    /// # Returns
    ///
    /// Handler using `Arachnea-Proxy-Country` and `country`.
    pub fn new() -> Self {
        Self {
            definition: default_country_parameter_definition(),
            routes: BTreeMap::new(),
            stop_on_match: true,
        }
    }

    /// Builds a country-routing handler from configuration.
    ///
    /// # Parameters
    ///
    /// - `config`: Serializable handler configuration.
    ///
    /// # Returns
    ///
    /// Configured country-routing handler.
    ///
    /// # Errors
    ///
    /// Returns an error when one configured route uses a direct node.
    pub fn from_config(config: &ParameterHandlerConfig) -> Result<Self> {
        let mut handler = Self::new();
        if let Some(parameter_name) = &config.parameter_name {
            handler.definition.name = parameter_name.clone();
        }
        if let Some(http_header) = &config.http_header {
            handler.definition.http_header = http_header.clone();
        }
        handler.definition.forward_header = config.forward_header;
        handler.stop_on_match = config.stop_on_match;
        for route in &config.routes {
            handler = handler.with_proxy_for_country(&route.value, route.proxy.clone())?;
        }
        Ok(handler)
    }

    /// Adds a country code to proxy mapping.
    ///
    /// # Parameters
    ///
    /// - `country`: Country code or region label.
    /// - `proxy`: Extra proxy appended when `country` matches.
    ///
    /// # Returns
    ///
    /// Updated handler.
    ///
    /// # Errors
    ///
    /// Returns an error when `proxy` is a direct node.
    pub fn with_proxy_for_country(
        mut self,
        country: impl AsRef<str>,
        proxy: ProxyNode,
    ) -> Result<Self> {
        validate_extra_proxy(&proxy)?;
        self.routes
            .insert(normalize_country(country.as_ref()), proxy);
        Ok(self)
    }
}

impl Default for CountryRoutingProxyHandler {
    /// Returns the default country-routing parameter handler.
    fn default() -> Self {
        Self::new()
    }
}

impl ProxyParameterHandler for CountryRoutingProxyHandler {
    /// Returns the country parameter definition.
    fn parameter_definitions(&self) -> Vec<ParameterDefinition> {
        vec![self.definition.clone()]
    }

    /// Appends the proxy mapped to the requested country when present.
    ///
    /// # Parameters
    ///
    /// - `parameters`: Client context containing optional country data.
    ///
    /// # Returns
    ///
    /// Handler decision with an extra proxy when a configured country matches.
    ///
    /// # Errors
    ///
    /// This implementation does not currently fail.
    fn proxy_from_parameters(
        &self,
        parameters: &ClientContext,
    ) -> Result<ParameterHandlerDecision> {
        let Some(country) = parameters.get_string(&self.definition.name) else {
            return Ok(ParameterHandlerDecision::continue_without_proxy());
        };
        Ok(self
            .routes
            .get(&normalize_country(country))
            .cloned()
            .map_or_else(ParameterHandlerDecision::continue_without_proxy, |proxy| {
                ParameterHandlerDecision::with_proxy(proxy, self.stop_on_match)
            }))
    }
}

/// Registry that merges parameter definitions from built-in and custom handlers.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ParameterRegistry {
    definitions: Vec<ParameterDefinition>,
}

impl ParameterRegistry {
    /// Creates a registry with built-in default parameters.
    ///
    /// # Returns
    ///
    /// Registry containing the default country parameter.
    pub fn with_defaults() -> Self {
        let mut registry = Self::default();
        registry.register(default_country_parameter_definition());
        registry
    }

    /// Registers or replaces one parameter definition.
    ///
    /// # Parameters
    ///
    /// - `definition`: Parameter definition to register.
    pub fn register(&mut self, definition: ParameterDefinition) {
        if let Some(existing) = self.definitions.iter_mut().find(|candidate| {
            candidate
                .http_header
                .eq_ignore_ascii_case(&definition.http_header)
                || candidate.name == definition.name
        }) {
            *existing = definition;
        } else {
            self.definitions.push(definition);
        }
    }

    /// Registers all definitions returned by one handler.
    ///
    /// # Parameters
    ///
    /// - `handler`: Parameter handler contributing definitions.
    pub fn register_handler(&mut self, handler: &dyn ProxyParameterHandler) {
        for definition in handler.parameter_definitions() {
            self.register(definition);
        }
    }

    /// Returns the merged parameter definition list.
    ///
    /// # Returns
    ///
    /// Registered parameter definitions.
    pub fn definitions(&self) -> &[ParameterDefinition] {
        &self.definitions
    }

    /// Converts this registry into a definition vector.
    ///
    /// # Returns
    ///
    /// Owned parameter definitions.
    pub fn into_definitions(self) -> Vec<ParameterDefinition> {
        self.definitions
    }
}

/// Builds one configured parameter handler.
///
/// # Parameters
///
/// - `config`: Serializable parameter handler configuration.
///
/// # Returns
///
/// Boxed parameter handler.
///
/// # Errors
///
/// Returns an error when the handler configuration is invalid.
pub fn build_parameter_handler(
    config: &ParameterHandlerConfig,
) -> Result<Box<dyn ProxyParameterHandler>> {
    match config.kind {
        ParameterHandlerKind::SmartDns => Ok(Box::new(SmartDnsProxyHandler::from_config(config)?)),
        ParameterHandlerKind::CountryRouting => {
            Ok(Box::new(CountryRoutingProxyHandler::from_config(config)?))
        }
    }
}

/// Validates configured parameter handlers.
///
/// # Parameters
///
/// - `handlers`: Handler configurations to validate.
///
/// # Errors
///
/// Returns an error when one route uses an invalid extra proxy.
pub fn validate_parameter_handler_configs(handlers: &[ParameterHandlerConfig]) -> Result<()> {
    for handler in handlers {
        for route in &handler.routes {
            validate_extra_proxy(&route.proxy)?;
        }
    }
    Ok(())
}

/// Builds a context from textual parameter pairs.
///
/// # Parameters
///
/// - `pairs`: Textual parameter pairs.
/// - `definitions`: Registered parameter definitions used as an allow-list.
///
/// # Returns
///
/// Client context containing recognized parameters.
pub fn context_from_parameter_pairs<'a>(
    pairs: impl IntoIterator<Item = (&'a str, &'a str)>,
    definitions: &[ParameterDefinition],
) -> ClientContext {
    let mut context = ClientContext::new();
    for (name, value) in pairs {
        if definitions.iter().any(|definition| definition.name == name) {
            context.insert(
                name.to_string(),
                ClientParameter::String(normalize_parameter_value(name, value)),
            );
        }
    }
    context
}

/// Returns the default country parameter definition.
///
/// # Returns
///
/// Parameter definition for `Arachnea-Proxy-Country`.
pub fn default_country_parameter_definition() -> ParameterDefinition {
    ParameterDefinition::new(
        PROXY_HEADER_PARAMETER_COUNTRY,
        PROXY_PARAMETER_COUNTRY,
        false,
    )
}

/// Normalizes textual parameter values for built-in conventions.
///
/// # Parameters
///
/// - `name`: Context parameter name.
/// - `value`: Raw textual value.
///
/// # Returns
///
/// Normalized textual parameter value.
pub fn normalize_parameter_value(name: &str, value: &str) -> String {
    if name == PROXY_PARAMETER_COUNTRY {
        normalize_country(value)
    } else {
        value.trim().to_string()
    }
}

/// Validates that a proxy node can be appended by a handler.
///
/// # Parameters
///
/// - `proxy`: Candidate extra proxy.
///
/// # Errors
///
/// Returns an error when `proxy` is a direct node.
fn validate_extra_proxy(proxy: &ProxyNode) -> Result<()> {
    if proxy.kind == TransportKind::Direct {
        return Err(ProxyError::Config(
            "parameter handlers cannot append a direct node".to_string(),
        ));
    }
    Ok(())
}

/// Normalizes a country-like routing value.
///
/// # Parameters
///
/// - `country`: Raw country code or region label.
///
/// # Returns
///
/// Trimmed upper-case country value.
fn normalize_country(country: &str) -> String {
    country.trim().to_ascii_uppercase()
}

/// Returns `true` for serde defaults.
///
/// # Returns
///
/// Always returns `true`.
fn default_true() -> bool {
    true
}
