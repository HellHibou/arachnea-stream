use async_trait::async_trait;

use crate::core::{ClientContext, ConnectRequest, ProxyError, Result, RouteDecision};

/// Extracts client parameters from an inbound protocol representation.
pub trait ClientParameterExtractor<Input>: Send + Sync {
    /// Extracts a context from a protocol-specific input.
    ///
    /// # Parameters
    ///
    /// - `input`: Protocol-specific input to inspect.
    ///
    /// # Returns
    ///
    /// Extracted client context.
    ///
    /// # Errors
    ///
    /// Returns an error when extraction rejects or cannot parse the input.
    fn extract(&self, input: &Input) -> Result<ClientContext>;
}

/// Validates and normalizes client parameters.
pub trait ClientParameterValidator: Send + Sync {
    /// Returns a filtered client context.
    ///
    /// # Parameters
    ///
    /// - `context`: Client context to validate.
    ///
    /// # Returns
    ///
    /// Validated and normalized context.
    ///
    /// # Errors
    ///
    /// Returns an error when the context violates validation policy.
    fn validate(&self, context: ClientContext) -> Result<ClientContext>;
}

/// Route extension point that can refine a selected route.
#[async_trait]
pub trait RoutePlugin: Send + Sync {
    /// Returns a replacement route or the original decision.
    ///
    /// # Parameters
    ///
    /// - `request`: Connect request being routed.
    /// - `decision`: Current route decision.
    ///
    /// # Returns
    ///
    /// Refined route decision.
    ///
    /// # Errors
    ///
    /// Returns an error when the plugin cannot produce a valid route decision.
    async fn refine_route(
        &self,
        request: &ConnectRequest,
        decision: RouteDecision,
    ) -> Result<RouteDecision>;
}

/// Marker trait for extension providers.
pub trait ExtensionProvider: Send + Sync {
    /// Returns a human-readable extension name.
    fn name(&self) -> &str;
}

/// Validator that rejects all parameters.
pub struct RejectAllParameters;

impl ClientParameterValidator for RejectAllParameters {
    /// Rejects any non-empty client context.
    ///
    /// # Parameters
    ///
    /// - `context`: Client context proposed by an inbound adapter.
    ///
    /// # Returns
    ///
    /// The unchanged empty context when no parameters were supplied.
    ///
    /// # Errors
    ///
    /// Returns an access-denied error when any parameter is present.
    fn validate(&self, context: ClientContext) -> Result<ClientContext> {
        if context.parameters.is_empty() {
            Ok(context)
        } else {
            Err(ProxyError::AccessDenied(
                "client parameters are not enabled".to_string(),
            ))
        }
    }
}
