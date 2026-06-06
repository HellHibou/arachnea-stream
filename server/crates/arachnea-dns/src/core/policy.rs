use crate::core::{QueryRequest, Record, RecordType};
use serde::{Deserialize, Serialize};

/// Local DNS records that can answer before cache or upstream resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalRecord {
    /// Domain name matched by this local record set.
    pub name: String,
    /// Records returned when the rule matches.
    pub records: Vec<Record>,
    /// Whether the name is treated as a wildcard suffix.
    pub wildcard: bool,
    /// Whether local answers have priority over upstream answers.
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockAction {
    Nxdomain,
    NoData,
    Refused,
    Empty,
    Address {
        ipv4: Option<std::net::Ipv4Addr>,
        ipv6: Option<std::net::Ipv6Addr>,
    },
}

/// Smart DNS rules independent from high-level profiles.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SmartDnsConfig {
    /// Ordered Smart DNS rules.
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
    Answer { records: Vec<Record> },
    Route { upstream: String },
    ProxyTarget { proxy_target: String },
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
    InternalDynamic,
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
    Allow,
    Block(BlockAction),
    Answer(Vec<Record>),
    Rewrite(String),
    Route(String),
    ProxyTarget(String),
    LogOnly,
}

/// Normalized exact/suffix/wildcard domain pattern.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DomainPattern {
    /// Original normalized pattern string.
    pub raw: String,
    /// Parsed matching strategy.
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DomainPatternKind {
    Exact(String),
    Suffix(String),
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicySource {
    Blocklist,
    LocalRecords,
    SmartDns,
    Rule,
}

/// NXDOMAIN behavior for failover strategies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NxdomainStrategy {
    TrustFirst,
    FallbackOnNxdomain,
    FallbackOnSuspiciousNxdomain,
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
