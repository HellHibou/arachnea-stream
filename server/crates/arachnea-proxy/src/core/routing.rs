use serde::{Deserialize, Serialize};

use crate::core::{
    dns_integration, ApplicationProtocol, ClientParameter, ConnectRequest, DestinationAddress,
    ProxyChain, ProxyError, ResolvedProxyConfig, Result,
};

/// Declarative route policy for selecting chains.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RoutePolicy {
    /// Default chain used when no rule matches.
    pub default_chain: Option<String>,
    /// Whether direct fallback is allowed after a proxy route fails.
    pub allow_direct_fallback: bool,
    /// Ordered routing rules evaluated before the default chain.
    #[serde(default)]
    pub rules: Vec<RouteRule>,
}

impl Default for RoutePolicy {
    /// Returns direct routing with direct fallback allowed.
    fn default() -> Self {
        Self {
            default_chain: Some("direct".to_string()),
            allow_direct_fallback: true,
            rules: Vec::new(),
        }
    }
}

impl RoutePolicy {
    /// Selects a route for a request.
    ///
    /// Smart DNS hints in the client context are consumed before declarative
    /// routing rules because they represent DNS decisions already computed for
    /// this hostname. A Smart DNS chain hint has priority over an egress-pool
    /// hint when both are present.
    ///
    /// # Parameters
    ///
    /// - `config`: Resolved configuration containing available chains.
    /// - `request`: Connect request being routed.
    ///
    /// # Returns
    ///
    /// Route decision with the selected chain.
    ///
    /// # Errors
    ///
    /// Returns an error when no route can be selected or the selected chain is
    /// not declared.
    pub fn decide(
        &self,
        config: &ResolvedProxyConfig,
        request: &ConnectRequest,
    ) -> Result<RouteDecision> {
        if let Some(pool_name) =
            dns_integration::smart_dns_egress_pool_from_context(&request.client_context)
        {
            if dns_integration::smart_dns_chain_from_context(&request.client_context).is_none() {
                let pool = config.egress_pool(pool_name).ok_or_else(|| {
                    ProxyError::RouteUnavailable(format!("egress pool '{pool_name}' not found"))
                })?;
                return Ok(RouteDecision {
                    selected_chain: format!("egress_pool:{}", pool.name),
                    chain: ProxyChain::pool(format!("egress_pool:{}", pool.name), &pool.name),
                });
            }
        }

        let chain_name = dns_integration::smart_dns_chain_from_context(&request.client_context)
            .or_else(|| {
                self.rules
                    .iter()
                    .find(|rule| rule.matches(request))
                    .map(|rule| rule.chain.as_str())
            })
            .or(self.default_chain.as_deref())
            .ok_or_else(|| {
                ProxyError::RouteUnavailable("no default chain configured".to_string())
            })?;
        let chain = config.chain(chain_name).ok_or_else(|| {
            ProxyError::RouteUnavailable(format!("chain '{chain_name}' not found"))
        })?;
        Ok(RouteDecision {
            selected_chain: chain.name.clone(),
            chain: chain.clone(),
        })
    }
}

/// One declarative routing rule.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RouteRule {
    /// Optional hostname suffix match such as `.example.com`.
    pub match_host_suffix: Option<String>,
    /// Optional exact target port match.
    pub match_port: Option<u16>,
    /// Optional application protocol match.
    pub match_protocol: Option<ApplicationProtocol>,
    /// Optional client context parameter name to match.
    pub match_client_parameter: Option<String>,
    /// Optional exact client context parameter value to match.
    pub match_client_value: Option<ClientParameter>,
    /// Chain selected when all configured matchers pass.
    pub chain: String,
}

impl RouteRule {
    /// Creates a route rule that selects a chain.
    ///
    /// # Parameters
    ///
    /// - `chain`: Chain name selected by the rule.
    ///
    /// # Returns
    ///
    /// Route rule without match predicates.
    pub fn to_chain(chain: impl Into<String>) -> Self {
        Self {
            match_host_suffix: None,
            match_port: None,
            match_protocol: None,
            match_client_parameter: None,
            match_client_value: None,
            chain: chain.into(),
        }
    }

    /// Creates a route rule that selects a chain for one application protocol.
    ///
    /// # Parameters
    ///
    /// - `protocol`: Application protocol that must match.
    /// - `chain`: Chain name selected by the rule.
    ///
    /// # Returns
    ///
    /// Route rule matching `protocol`.
    pub fn for_protocol(protocol: ApplicationProtocol, chain: impl Into<String>) -> Self {
        Self {
            match_host_suffix: None,
            match_port: None,
            match_protocol: Some(protocol),
            match_client_parameter: None,
            match_client_value: None,
            chain: chain.into(),
        }
    }

    /// Returns true when this rule matches a request.
    ///
    /// # Parameters
    ///
    /// - `request`: Connect request to evaluate.
    ///
    /// # Returns
    ///
    /// `true` when all configured predicates match.
    pub fn matches(&self, request: &ConnectRequest) -> bool {
        if let Some(suffix) = &self.match_host_suffix {
            let host = match &request.destination.address {
                DestinationAddress::Host(host) => host.as_str(),
                DestinationAddress::Ip(_) => return false,
            };
            if !host.ends_with(suffix) {
                return false;
            }
        }

        if let Some(port) = self.match_port {
            if request.destination.port != port {
                return false;
            }
        }

        if let Some(protocol) = &self.match_protocol {
            if &request.destination.protocol != protocol {
                return false;
            }
        }

        if let Some(parameter) = &self.match_client_parameter {
            let Some(value) = request.client_context.get(parameter) else {
                return false;
            };
            if let Some(expected) = &self.match_client_value {
                if value != expected {
                    return false;
                }
            }
        }

        true
    }
}

/// Effective route selected for a connection request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RouteDecision {
    /// Selected chain name.
    pub selected_chain: String,
    /// Chain to execute.
    pub chain: ProxyChain,
}
