//! Core DNS resolution module for Arachnea DNS.
//!
//! This crate owns configuration, policies, caching, upstream selection and
//! reusable resolution APIs. Server-specific socket handling lives behind the
//! `server` feature.

mod cache;
mod config;
mod error;
#[cfg(any(feature = "dnssec", feature = "doq"))]
mod hickory;
mod policy;
mod protocol;
#[cfg(feature = "recursive")]
mod recursive;
mod resolver;
mod transport;

pub use cache::{Cache, CacheState, CoreStats};
pub use config::{
    CacheConfig, CoreConfig, CoreConfigBuilder, DnssecState, EcsPolicy, PrivacyConfig,
    ResolutionMode, ResolvedConfig, ResolverConfig, SecurityConfig, Transport, Upstream,
    UpstreamEndpoint, UpstreamStrategy, UsageProfile,
};
pub use error::{DnsError, Result};
pub use policy::{
    BlockAction, BlockRule, DomainPattern, DomainPatternKind, LocalRecord, NxdomainStrategy,
    PolicyDecision, PolicySource, ProxyTarget, ProxyTargetMode, Rule, RuleAction, SmartDnsAction,
    SmartDnsConfig, SmartDnsRule,
};
pub use protocol::{
    normalize_name, record_to_hickory, records_to_hickory, Answer, AnswerMetadata, DnsClass,
    QueryRequest, Record, RecordData, RecordType,
};
pub use resolver::ArachneaDnsCore;

#[cfg(test)]
#[path = "tests/core_tests.rs"]
mod core_tests;
