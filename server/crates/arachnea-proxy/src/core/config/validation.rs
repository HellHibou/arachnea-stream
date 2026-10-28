//! Configuration validation for upstreams, pools and chains.

use crate::core::{
    EgressPool, ProxyChain, ProxyError, ProxyNode, Result, SecurityPolicy, TransportKind,
};
use std::collections::HashSet;

/// Validates that declared upstream names are non-empty and unique.
///
/// # Arguments
///
/// - `upstreams`: Upstream node declarations from the configuration.
///
/// # Errors
///
/// Returns an error when a name is empty or appears more than once.
pub(super) fn validate_unique_upstream_names(upstreams: &[ProxyNode]) -> Result<()> {
    let mut names = HashSet::new();
    for upstream in upstreams {
        if upstream.name.trim().is_empty() {
            return Err(ProxyError::Config(
                "upstream node names cannot be empty".to_string(),
            ));
        }
        if !names.insert(upstream.name.as_str()) {
            return Err(ProxyError::Config(format!(
                "upstream node '{}' is declared more than once",
                upstream.name
            )));
        }
    }
    Ok(())
}

/// Validates declared egress pools and their upstream references.
///
/// # Arguments
///
/// - `pools`: Egress pool declarations from the configuration.
/// - `security`: Security policy used for upstream TLS checks.
///
/// # Errors
///
/// Returns an error when a pool name is empty or duplicated, a pool has no
/// upstreams, or one pool member is missing or invalid.
pub(super) fn validate_egress_pools(pools: &[EgressPool], security: &SecurityPolicy) -> Result<()> {
    let mut pool_names = HashSet::new();
    for pool in pools {
        if pool.name.trim().is_empty() {
            return Err(ProxyError::Config(
                "egress pool names cannot be empty".to_string(),
            ));
        }
        if !pool_names.insert(pool.name.as_str()) {
            return Err(ProxyError::Config(format!(
                "egress pool '{}' is declared more than once",
                pool.name
            )));
        }
        if pool.proxy_nodes.is_empty() {
            return Err(ProxyError::Config(format!(
                "egress pool '{}' has no upstreams",
                pool.name
            )));
        }
        if pool.test_batch_size == 0 {
            return Err(ProxyError::Config(format!(
                "egress pool '{}' test_batch_size must be greater than 0",
                pool.name
            )));
        }

        let mut member_names = HashSet::new();
        for upstream in &pool.proxy_nodes {
            if upstream.name.trim().is_empty() {
                return Err(ProxyError::Config(format!(
                    "egress pool '{}' contains an empty upstream reference",
                    pool.name
                )));
            }
            if !member_names.insert(upstream.name.as_str()) {
                return Err(ProxyError::Config(format!(
                    "egress pool '{}' references upstream '{}' more than once",
                    pool.name, upstream.name
                )));
            }
            if matches!(
                upstream.kind,
                TransportKind::Direct | TransportKind::ProxyPool
            ) {
                return Err(ProxyError::Config(format!(
                    "egress pool '{}' upstream '{}' must be a concrete proxy node",
                    pool.name, upstream.name
                )));
            }
            validate_proxy_node(upstream, security)?;
        }
    }
    Ok(())
}

/// Validates node-level constraints inside a proxy chain.
///
/// # Arguments
///
/// - `chain`: Chain to validate.
/// - `security`: Security policy used for upstream TLS checks.
/// - `pool_names`: Declared egress pool names that chain markers may reference.
///
/// # Errors
///
/// Returns an error when nodes are duplicated, incompatible, missing endpoints,
/// or unsafe according to `security`.
pub(super) fn validate_chain_nodes(
    chain: &ProxyChain,
    security: &SecurityPolicy,
    pool_names: &HashSet<&str>,
) -> Result<()> {
    let has_direct = chain
        .nodes
        .iter()
        .any(|node| node.kind == crate::core::TransportKind::Direct);
    if has_direct && chain.nodes.len() > 1 {
        return Err(ProxyError::Config(format!(
            "chain '{}' mixes direct with proxy nodes",
            chain.name
        )));
    }

    let mut node_names = HashSet::new();
    for node in &chain.nodes {
        if node.name.trim().is_empty() {
            return Err(ProxyError::Config(format!(
                "chain '{}' contains a node with an empty name",
                chain.name
            )));
        }
        if !node_names.insert(node.name.as_str()) {
            return Err(ProxyError::Config(format!(
                "chain '{}' declares node '{}' more than once",
                chain.name, node.name
            )));
        }

        if node.kind == TransportKind::ProxyPool {
            if node.endpoint.is_some() {
                return Err(ProxyError::Config(format!(
                    "proxy pool marker '{}' must not declare an endpoint",
                    node.name
                )));
            }
            if !pool_names.contains(node.name.as_str()) {
                return Err(ProxyError::Config(format!(
                    "chain '{}' references undeclared egress pool '{}'",
                    chain.name, node.name
                )));
            }
        } else {
            validate_proxy_node(node, security)?;
        }
    }

    Ok(())
}

/// Validates one concrete proxy node declaration.
///
/// # Arguments
///
/// - `node`: Proxy node to validate.
/// - `security`: Security policy used for upstream TLS checks.
///
/// # Errors
///
/// Returns an error when the node endpoint is invalid or violates TLS policy.
fn validate_proxy_node(node: &ProxyNode, security: &SecurityPolicy) -> Result<()> {
    match node.kind {
        TransportKind::Direct => {
            if node.endpoint.is_some() {
                return Err(ProxyError::Config(format!(
                    "direct node '{}' must not declare an endpoint",
                    node.name
                )));
            }
        }
        TransportKind::ProxyPool => {
            return Err(ProxyError::Config(format!(
                "proxy pool marker '{}' is not a concrete proxy node",
                node.name
            )));
        }
        _ => {
            let endpoint = node.endpoint.as_deref().ok_or_else(|| {
                ProxyError::Config(format!("node '{}' requires an endpoint", node.name))
            })?;
            let endpoint_destination = crate::core::Destination::from_authority(
                endpoint,
                crate::core::ApplicationProtocol::Tcp,
            )
            .map_err(|_| {
                ProxyError::Config(format!(
                    "node '{}' has an invalid endpoint '{}'",
                    node.name, endpoint
                ))
            })?;
            if endpoint_destination.port == 0 {
                return Err(ProxyError::Config(format!(
                    "node '{}' endpoint port cannot be 0",
                    node.name
                )));
            }
        }
    }

    if !security.allow_insecure_upstreams
        && security.verify_upstream_tls
        && matches!(node.kind, TransportKind::HttpsProxy)
        && !node.verify_tls
    {
        return Err(ProxyError::Config(format!(
            "https upstream '{}' disables tls verification",
            node.name
        )));
    }
    Ok(())
}
