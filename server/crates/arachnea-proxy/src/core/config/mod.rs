//! Proxy configuration types and their associated implementations.

mod builder;
mod name_resolution;
mod proxy_config;
mod proxy_profile;
mod resolved;
mod system_relay;
mod timeout;
mod usage_profile;
mod validation;

pub use builder::ProxyConfigBuilder;
pub use name_resolution::NameResolutionMode;
pub use proxy_config::ProxyConfig;
pub use proxy_profile::ProxyProfile;
pub use resolved::ResolvedProxyConfig;
pub use timeout::TimeoutConfig;
pub use usage_profile::UsageProfile;
