use crate::core::{QueryRequest, Record, RecordType};
use serde::{Deserialize, Serialize};

/// Local DNS records that can answer before cache or upstream resolution.
/// 
/// This struct defines domain names that are answered locally by the resolver
/// without querying upstream DNS servers. Local records are useful for testing,
/// development, or overriding DNS responses for specific domains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalRecord {
    /// Domain name matched by this local record set.
    /// 
    /// The domain name for which this local record set provides answers.
    /// If wildcard is true, this acts as a suffix match.
    pub name: String,
    /// Records returned when the rule matches.
    /// 
    /// The resource records that are returned when a query matches this
    /// local record's domain name and record type.
    pub records: Vec<Record>,
    /// Whether the name is treated as a wildcard suffix.
    /// 
    /// If true, this local record matches any domain that ends with the
    /// specified name (e.g., "example.com" would match "www.example.com").
    pub wildcard: bool,
    /// Whether local answers have priority over upstream answers.
    /// 
    /// If true, local records are returned even when upstream servers
    /// could provide answers. If false, upstream answers are preferred.
    pub priority_over_upstream: bool,
}

/// Domain blocklist rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockRule {
    /// Rule name used in explanations.
    pub name: String,
    /// Domain match pattern.
    pub pattern: DomainPattern,
    /// Blocking behavior.
    pub action: BlockAction,
    /// Whether the rule is active.
    pub enabled: bool,
}

/// Action returned when a block rule matches.
/// 
/// This enum defines the different ways a block rule can respond to
/// DNS queries for blocked domains. Different actions provide different
/// behaviors for handling blocked content.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockAction {
    /// Nxdomain,
    /// 
    /// Respond with NXDOMAIN, indicating that the domain does not exist.
    /// This is the most common blocking behavior.
    Nxdomain,
    /// NoData,
    /// 
    /// Respond with NODATA, indicating that the domain exists but has
    /// no records of the requested type.
    NoData,
    /// Refused,
    /// 
    /// Respond with REFUSED, indicating that the server refuses to
    /// answer the query for policy reasons.
    Refused,
    /// Empty,
    /// 
    /// Respond with an empty answer (no records) but without an error.
    Empty,
    /// Address,
    /// 
    /// Respond with specific IP addresses instead of the real ones.
    /// This can be used to redirect blocked domains to a specific server
    /// (e.g., a blocking page or local service).
    Address {
        /// IPv4 address to return for A queries.
        ipv4: Option<std::net::Ipv4Addr>,
        /// IPv6 address to return for AAAA queries.
        ipv6: Option<std::net::Ipv6Addr>,
    },
}

/// Smart DNS rules independent from high-level profiles.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SmartDnsConfig {
    /// Ordered Smart DNS rules evaluated at runtime.
    pub rules: Vec<SmartDnsRule>,
}

/// One Smart DNS matching rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartDnsRule {
    /// Rule name used in explanations.
    pub name: String,
    /// Domain match pattern.
    pub pattern: DomainPattern,
    /// Smart DNS action.
    pub action: SmartDnsAction,
}

/// Action for Smart DNS routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmartDnsAction {
    /// Return the specified records as the answer.
    Answer {
        /// Records to return in the response.
        records: Vec<Record>,
    },
    /// Route the query to a named upstream.
    Route {
        /// Name of the upstream target.
        upstream: String,
    },
    /// Route the query to a named proxy target.
    ProxyTarget {
        /// Name of the proxy target.
        proxy_target: String,
    },
}

/// Proxy target referenced by Smart DNS rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyTarget {
    /// Target name.
    pub name: String,
    /// Target mode.
    pub mode: ProxyTargetMode,
    /// Records returned until dynamic proxy selection exists.
    pub fallback_records: Vec<Record>,
}

/// Proxy target selection mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyTargetMode {
    /// Dynamic internal proxy selection is used.
    InternalDynamic,
    /// Static fallback records are always returned.
    Static,
}

/// Advanced ordered policy rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// Rule name used in explanations.
    pub name: String,
    /// Domain match pattern.
    pub pattern: DomainPattern,
    /// Rule action.
    pub action: RuleAction,
    /// Whether the rule is active.
    pub enabled: bool,
}

/// Action for advanced rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleAction {
    /// Allow the query to proceed normally.
    Allow,
    /// Block the query with the given action.
    Block(BlockAction),
    /// Return the specified records as the answer.
    Answer(Vec<Record>),
    /// Rewrite the queried domain name.
    Rewrite(String),
    /// Route the query to a named upstream.
    Route(String),
    /// Route the query to a named proxy target.
    ProxyTarget(String),
    /// Log the query without taking action.
    LogOnly,
}

/// Normalized exact/suffix/wildcard domain pattern.
/// 
/// This struct represents a domain matching pattern that can match domains
/// exactly, by suffix, or using wildcards. Patterns are normalized to
/// lowercase with trailing dots removed for consistent matching.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DomainPattern {
    /// Original normalized pattern string.
    /// 
    /// The pattern string after normalization (lowercase, no trailing dot).
    pub raw: String,
    /// Parsed matching strategy.
    /// 
    /// The specific type of matching to perform with this pattern.
    pub kind: DomainPatternKind,
}

impl DomainPattern {
    /// Parses a domain pattern.
    ///
    /// # Parameters
    ///
    /// - `value`: Pattern text using exact, suffix, or wildcard syntax.
    ///
    /// # Returns
    ///
    /// Parsed domain pattern.
    pub fn new(value: &str) -> Self {
        let raw = crate::core::normalize_name(value);
        let kind = if raw.starts_with("*.") {
            DomainPatternKind::WildcardSuffix(raw.trim_start_matches("*.").to_owned())
        } else if raw.starts_with('.') {
            DomainPatternKind::Suffix(raw.trim_start_matches('.').to_owned())
        } else {
            DomainPatternKind::Exact(raw.clone())
        };

        Self { raw, kind }
    }

    /// Returns true when the pattern matches the provided DNS name.
    ///
    /// # Parameters
    ///
    /// - `name`: DNS name to test.
    ///
    /// # Returns
    ///
    /// `true` when `name` matches this pattern.
    pub fn matches(&self, name: &str) -> bool {
        let name = crate::core::normalize_name(name);
        match &self.kind {
            DomainPatternKind::Exact(exact) => &name == exact,
            DomainPatternKind::Suffix(suffix) => {
                name == *suffix || name.ends_with(&format!(".{suffix}"))
            }
            DomainPatternKind::WildcardSuffix(suffix) => name.ends_with(&format!(".{suffix}")),
        }
    }
}

impl From<&str> for DomainPattern {
    /// Creates an exact domain pattern from a string.
    ///
    /// # Parameters
    ///
    /// - `value`: Domain name used for exact matching.
    ///
    /// # Returns
    ///
    /// Exact domain pattern.
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

/// Kind of domain pattern.
/// 
/// This enum defines the different types of domain matching that can be
/// performed by domain patterns. Each variant specifies how the pattern
/// should be interpreted when matching against domain names.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DomainPatternKind {
    /// Exact(String),
    /// 
    /// The pattern must match the domain name exactly, character for character.
    Exact(String),
    /// Suffix(String),
    /// 
    /// The pattern matches any domain name that ends with the specified suffix.
    /// For example, ".example.com" would match "www.example.com" and
    /// "api.example.com".
    Suffix(String),
    /// WildcardSuffix(String),
    /// 
    /// The pattern matches any domain name that ends with the specified suffix,
    /// but the suffix must be preceded by at least one label. For example,
    /// "*.example.com" would match "www.example.com" but not "example.com".
    WildcardSuffix(String),
}

/// Explanation of the policy that produced an answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDecision {
    /// Policy layer that made the decision.
    pub source: PolicySource,
    /// Optional rule name.
    pub rule_name: Option<String>,
    /// Human-readable action label.
    pub action: String,
}

/// Policy layer that produced a decision.
/// 
/// This enum identifies which type of policy rule was responsible for
/// a particular DNS resolution decision. This information is included
/// in metadata to provide transparency about how queries were handled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicySource {
    /// Blocklist,
    /// 
    /// The decision was made by a blocklist rule that matches blocked
    /// domain patterns.
    Blocklist,
    /// LocalRecords,
    /// 
    /// The decision was made by a local record configuration that
    /// provides predefined answers for specific domains.
    LocalRecords,
    /// SmartDns,
    /// 
    /// The decision was made by a Smart DNS rule that provides advanced
    /// routing or answering behavior based on domain patterns.
    SmartDns,
    /// Rule,
    /// 
    /// The decision was made by an advanced policy rule that can perform
    /// various actions like allowing, blocking, answering, or routing
    /// queries.
    Rule,
}

/// NXDOMAIN behavior for failover strategies.
/// 
/// This enum defines how the resolver should handle NXDOMAIN (non-existent
/// domain) responses when using multiple upstream servers with failover
/// or consensus strategies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NxdomainStrategy {
    /// TrustFirst,
    /// 
    /// Accept the first upstream's NXDOMAIN response without questioning
    /// it or trying other upstreams.
    TrustFirst,
    /// FallbackOnNxdomain,
    /// 
    /// When an upstream returns NXDOMAIN, try the next upstream in the
    /// list to see if it can provide a positive answer.
    FallbackOnNxdomain,
    /// FallbackOnSuspiciousNxdomain,
    /// 
    /// Similar to FallbackOnNxdomain, but only fall back when the
    /// NXDOMAIN response seems suspicious (e.g., from upstreams that
    /// might be censoring or manipulating responses).
    FallbackOnSuspiciousNxdomain,
    /// Consensus,
    /// 
    /// Require multiple upstreams to agree on an NXDOMAIN response
    /// before accepting it as definitive.
    Consensus,
}

/// Filters records by DNS record type.
///
/// # Parameters
///
/// - `records`: Candidate records.
/// - `record_type`: Record type requested by the query.
///
/// # Returns
///
/// Records whose data type matches `record_type`.
pub(crate) fn matching_records(records: &[Record], record_type: RecordType) -> Vec<Record> {
    records
        .iter()
        .filter(|record| {
            record.data.record_type() == record_type
                || record.data.record_type() == RecordType::CNAME
        })
        .cloned()
        .collect()
}

/// Returns local records that match a query.
///
/// # Parameters
///
/// - `local`: Local record rule to evaluate.
/// - `query`: Query request being resolved.
///
/// # Returns
///
/// Matching records when the local rule applies to `query`.
pub(crate) fn local_records_for_query(
    local: &LocalRecord,
    query: &QueryRequest,
) -> Option<Vec<Record>> {
    let pattern = if local.wildcard {
        DomainPattern::new(&format!("*.{}", local.name))
    } else {
        DomainPattern::new(&local.name)
    };

    pattern
        .matches(&query.name)
        .then(|| matching_records(&local.records, query.record_type))
}
