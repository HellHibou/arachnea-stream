use serde::{Deserialize, Serialize};

use crate::core::{ClientContext, ClientParameter};

/// Client-context parameter carrying a Smart DNS-selected proxy chain.
pub const PROXY_PARAMETER_SMART_DNS_CHAIN: &str = "smart_dns_chain";

/// Client-context parameter carrying a Smart DNS-selected egress pool.
pub const PROXY_PARAMETER_SMART_DNS_EGRESS_POOL: &str = "smart_dns_egress_pool";

/// Optional Smart DNS hint consumed by route plugins.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SmartDnsRouteHint {
    /// Optional chain name recommended by DNS policy.
    pub chain: Option<String>,
    /// Optional egress pool name recommended by DNS policy.
    pub egress_pool: Option<String>,
}

impl SmartDnsRouteHint {
    /// Creates a Smart DNS route hint from optional routing parts.
    ///
    /// # Parameters
    ///
    /// - `chain`: Optional proxy chain name selected by Smart DNS.
    /// - `egress_pool`: Optional egress pool selected by Smart DNS.
    ///
    /// # Returns
    ///
    /// Smart DNS hint ready to attach to a client context.
    pub fn new(chain: Option<String>, egress_pool: Option<String>) -> Self {
        Self { chain, egress_pool }
    }

    /// Reads a Smart DNS route hint from a client context.
    ///
    /// # Parameters
    ///
    /// - `context`: Client context that may contain Smart DNS parameters.
    ///
    /// # Returns
    ///
    /// Route hint when at least one Smart DNS parameter is present.
    pub fn from_context(context: &ClientContext) -> Option<Self> {
        let chain = context
            .get_string(PROXY_PARAMETER_SMART_DNS_CHAIN)
            .map(str::to_owned);
        let egress_pool = context
            .get_string(PROXY_PARAMETER_SMART_DNS_EGRESS_POOL)
            .map(str::to_owned);
        if chain.is_none() && egress_pool.is_none() {
            None
        } else {
            Some(Self { chain, egress_pool })
        }
    }

    /// Inserts this Smart DNS route hint into a client context.
    ///
    /// # Parameters
    ///
    /// - `context`: Client context receiving the hint parameters.
    pub fn insert_into_context(&self, context: &mut ClientContext) {
        if let Some(chain) = &self.chain {
            context.insert(
                PROXY_PARAMETER_SMART_DNS_CHAIN,
                ClientParameter::String(chain.clone()),
            );
        }
        if let Some(egress_pool) = &self.egress_pool {
            context.insert(
                PROXY_PARAMETER_SMART_DNS_EGRESS_POOL,
                ClientParameter::String(egress_pool.clone()),
            );
        }
    }
}

/// Returns the Smart DNS-selected chain from a client context.
///
/// # Parameters
///
/// - `context`: Client context that may contain a Smart DNS route hint.
///
/// # Returns
///
/// Chain name selected by Smart DNS, when one was supplied.
pub fn smart_dns_chain_from_context(context: &ClientContext) -> Option<&str> {
    context.get_string(PROXY_PARAMETER_SMART_DNS_CHAIN)
}

/// Returns the Smart DNS-selected egress pool from a client context.
///
/// # Parameters
///
/// - `context`: Client context that may contain a Smart DNS route hint.
///
/// # Returns
///
/// Egress pool name selected by Smart DNS, when one was supplied.
pub fn smart_dns_egress_pool_from_context(context: &ClientContext) -> Option<&str> {
    context.get_string(PROXY_PARAMETER_SMART_DNS_EGRESS_POOL)
}
