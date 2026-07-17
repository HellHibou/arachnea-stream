use crate::core::{
    policy::{local_records_for_query, matching_records},
    transport::{resolve_tcp, resolve_udp},
    Answer, AnswerMetadata, BlockAction, Cache, CacheState, CoreConfig, CoreConfigBuilder,
    CoreStats, DnsError, DnssecState, PolicyDecision, PolicySource, QueryRequest, Record,
    RecordData, RecordType, ResolutionMode, ResolvedConfig, Result, RuleAction, SmartDnsAction,
    Transport, Upstream, UpstreamEndpoint,
};
use std::{net::IpAddr, sync::Arc, time::Duration};
use tokio::{sync::RwLock, time::timeout};
use tracing::{debug, warn};

/// Main reusable DNS resolver.
///
/// This struct represents the core DNS resolver that handles DNS queries,
/// applies policies, manages caching, and communicates with upstream
/// DNS servers. It is the central component of the DNS resolution system.
pub struct ArachneaDnsCore {
    /// Resolved configuration.
    ///
    /// The complete, validated configuration for this resolver instance,
    /// including all settings and derived values.
    config: ResolvedConfig,
    /// DNS cache instance.
    ///
    /// The in-memory cache that stores both positive and negative DNS
    /// responses to improve performance and reduce upstream queries.
    cache: Cache,
    /// Runtime statistics.
    ///
    /// Counters and metrics that track the operation of this resolver
    /// instance, including request counts, cache hits/misses, and errors.
    stats: Arc<RwLock<CoreStats>>,
}

impl ArachneaDnsCore {
    /// Creates a new core instance with its own cache and stats.
    ///
    /// # Parameters
    ///
    /// - `config`: DNS core configuration to resolve and validate.
    ///
    /// # Returns
    ///
    /// DNS core instance.
    ///
    /// # Errors
    ///
    /// Returns an error when configuration validation fails.
    pub fn new(config: CoreConfig) -> Result<Self> {
        let config = config.resolve_effective_config()?;
        Ok(Self {
            cache: Cache::new(config.config.cache.clone()),
            config,
            stats: Arc::new(RwLock::new(CoreStats::default())),
        })
    }

    /// Starts a builder for core construction.
    pub fn builder() -> CoreConfigBuilder {
        CoreConfigBuilder::default()
    }

    /// Returns the effective config used by this instance.
    pub fn effective_config(&self) -> &ResolvedConfig {
        &self.config
    }

    /// Resolves a single DNS query asynchronously.
    ///
    /// # Parameters
    ///
    /// - `query`: DNS query to resolve.
    ///
    /// # Returns
    ///
    /// DNS answer.
    ///
    /// # Errors
    ///
    /// Returns an error when policy, cache, or upstream resolution fails.
    pub async fn resolve(&self, query: QueryRequest) -> Result<Answer> {
        self.stats.write().await.requests += 1;
        let query = QueryRequest {
            name: crate::core::normalize_name(&query.name),
            ..query
        };
        debug!(name = %query.name, record_type = ?query.record_type, "resolving DNS query");

        if let Some(answer) = self.apply_policy(&query).await? {
            return Ok(self.annotate_answer(answer));
        }

        let key = self.cache_key(&query);
        if let Some((answer, state)) = self.cache.get(&key).await {
            if matches!(state, CacheState::Hit | CacheState::Stale) {
                self.stats.write().await.cache_hits += 1;
            }
            debug!(name = %query.name, cache = ?state, "DNS cache hit");
            return Ok(self.annotate_answer(answer));
        }

        if let Some(error) = self.cache.get_negative(&key).await {
            self.stats.write().await.negative_cache_hits += 1;
            return Err(error);
        }

        if let Some(error) = self.cache.get_aggressive_negative(&query).await {
            self.stats.write().await.aggressive_negative_cache_hits += 1;
            return Err(error);
        }

        if self.config.config.cache.enabled {
            self.stats.write().await.cache_misses += 1;
        }

        match self.resolve_upstream(query.clone()).await {
            Ok(answer) => {
                let answer = self.annotate_answer(answer);
                self.cache.put(key, answer.clone()).await;
                Ok(answer)
            }
            Err(DnsError::Nxdomain(name)) => {
                self.cache
                    .put_negative(key, DnsError::Nxdomain(name.clone()))
                    .await;
                Err(DnsError::Nxdomain(name))
            }
            Err(DnsError::NoData { name, record_type }) => {
                self.cache
                    .put_negative(
                        key,
                        DnsError::NoData {
                            name: name.clone(),
                            record_type,
                        },
                    )
                    .await;
                Err(DnsError::NoData { name, record_type })
            }
            Err(error) => Err(error),
        }
    }

    /// Adds runtime metadata to an answer.
    ///
    /// # Parameters
    ///
    /// - `answer`: Answer produced by policy, cache, or upstream resolution.
    ///
    /// # Returns
    ///
    /// Answer with resolver metadata applied.
    fn annotate_answer(&self, mut answer: Answer) -> Answer {
        answer.metadata.dnssec = self.config.config.security.dnssec;
        answer
    }

    /// Resolves A and AAAA records as one logical operation.
    ///
    /// # Parameters
    ///
    /// - `name`: DNS name to resolve.
    ///
    /// # Returns
    ///
    /// Resolved IP addresses.
    ///
    /// # Errors
    ///
    /// Returns an error when an upstream transport fails or neither A nor AAAA
    /// records can be resolved.
    pub async fn resolve_ip(&self, name: &str) -> Result<Vec<IpAddr>> {
        let (a, aaaa) = tokio::join!(
            self.resolve(QueryRequest::new(name, RecordType::A)),
            self.resolve(QueryRequest::new(name, RecordType::AAAA)),
        );

        let mut ips = Vec::new();
        let mut last_negative = None;
        for result in [a, aaaa] {
            match result {
                Ok(answer) => {
                    for record in answer.records {
                        match record.data {
                            RecordData::A { address } => ips.push(IpAddr::V4(address)),
                            RecordData::AAAA { address } => ips.push(IpAddr::V6(address)),
                            _ => {}
                        }
                    }
                }
                Err(error @ (DnsError::NoData { .. } | DnsError::Nxdomain(_))) => {
                    last_negative = Some(error);
                }
                Err(error) => return Err(error),
            }
        }

        if ips.is_empty() {
            return Err(last_negative.unwrap_or_else(|| DnsError::NoData {
                name: crate::core::normalize_name(name),
                record_type: RecordType::A,
            }));
        }

        Ok(ips)
    }

    /// Resolves several queries in order and returns per-query results.
    ///
    /// # Parameters
    ///
    /// - `queries`: Queries to resolve sequentially.
    ///
    /// # Returns
    ///
    /// Per-query resolution results.
    pub async fn resolve_many(&self, queries: Vec<QueryRequest>) -> Vec<Result<Answer>> {
        let mut results = Vec::with_capacity(queries.len());
        for query in queries {
            results.push(self.resolve(query).await);
        }
        results
    }

    /// Clears the instance-owned cache.
    pub async fn flush_cache(&self) {
        self.cache.clear().await;
    }

    /// Returns a snapshot of runtime counters.
    pub async fn stats(&self) -> CoreStats {
        self.stats.read().await.clone()
    }

    /// Builds the cache key for a query under current privacy/security policy.
    ///
    /// # Parameters
    ///
    /// - `query`: Query whose cache key is requested.
    ///
    /// # Returns
    ///
    /// Cache key including policy dimensions that affect answers.
    pub(crate) fn cache_key(&self, query: &QueryRequest) -> crate::core::cache::CacheKey {
        crate::core::cache::CacheKey::new(
            query,
            self.config.config.privacy.ecs.clone(),
            self.config.config.security.dnssec,
        )
    }

    /// Applies local resolver policies before upstream resolution.
    ///
    /// # Parameters
    ///
    /// - `query`: Normalized query to evaluate against configured policies.
    ///
    /// # Returns
    ///
    /// Optional policy answer. `None` means upstream resolution should continue.
    ///
    /// # Errors
    ///
    /// Returns an error when a policy blocks the query or references invalid
    /// configuration.
    async fn apply_policy(&self, query: &QueryRequest) -> Result<Option<Answer>> {
        for block in self
            .config
            .config
            .blocklists
            .iter()
            .filter(|rule| rule.enabled)
        {
            if block.pattern.matches(&query.name) {
                return block_answer(
                    query,
                    &block.action,
                    Some(block.name.clone()),
                    PolicySource::Blocklist,
                );
            }
        }

        for local in &self.config.config.local_records {
            if let Some(records) = local_records_for_query(local, query) {
                return Ok(Some(answer_from_records(
                    query,
                    records,
                    Some(PolicyDecision {
                        source: PolicySource::LocalRecords,
                        rule_name: Some(local.name.clone()),
                        action: "answer".to_owned(),
                    }),
                )));
            }
        }

        for smart_rule in &self.config.config.smart_dns.rules {
            if smart_rule.pattern.matches(&query.name) {
                match &smart_rule.action {
                    SmartDnsAction::Answer { records } => {
                        return Ok(Some(answer_from_records(
                            query,
                            matching_records(records, query.record_type),
                            Some(PolicyDecision {
                                source: PolicySource::SmartDns,
                                rule_name: Some(smart_rule.name.clone()),
                                action: "answer".to_owned(),
                            }),
                        )));
                    }
                    SmartDnsAction::ProxyTarget { proxy_target } => {
                        let Some(target) = self
                            .config
                            .config
                            .proxy_targets
                            .iter()
                            .find(|target| target.name == *proxy_target)
                        else {
                            return Err(DnsError::InvalidConfig(format!(
                                "unknown proxy target {proxy_target}"
                            )));
                        };
                        return Ok(Some(answer_from_records(
                            query,
                            matching_records(&target.fallback_records, query.record_type),
                            Some(PolicyDecision {
                                source: PolicySource::SmartDns,
                                rule_name: Some(smart_rule.name.clone()),
                                action: "proxy_target".to_owned(),
                            }),
                        )));
                    }
                    SmartDnsAction::Route { .. } => {}
                }
            }
        }

        for rule in self.config.config.rules.iter().filter(|rule| rule.enabled) {
            if !rule.pattern.matches(&query.name) {
                continue;
            }

            match &rule.action {
                RuleAction::Allow | RuleAction::LogOnly | RuleAction::Route(_) => return Ok(None),
                RuleAction::Block(action) => {
                    return block_answer(
                        query,
                        action,
                        Some(rule.name.clone()),
                        PolicySource::Rule,
                    );
                }
                RuleAction::Answer(records) => {
                    return Ok(Some(answer_from_records(
                        query,
                        matching_records(records, query.record_type),
                        Some(PolicyDecision {
                            source: PolicySource::Rule,
                            rule_name: Some(rule.name.clone()),
                            action: "answer".to_owned(),
                        }),
                    )));
                }
                RuleAction::Rewrite(_name) => return Ok(None),
                RuleAction::ProxyTarget(target) => {
                    let Some(target) = self
                        .config
                        .config
                        .proxy_targets
                        .iter()
                        .find(|candidate| candidate.name == *target)
                    else {
                        return Err(DnsError::InvalidConfig(format!(
                            "unknown proxy target {target}"
                        )));
                    };
                    return Ok(Some(answer_from_records(
                        query,
                        matching_records(&target.fallback_records, query.record_type),
                        Some(PolicyDecision {
                            source: PolicySource::Rule,
                            rule_name: Some(rule.name.clone()),
                            action: "proxy_target".to_owned(),
                        }),
                    )));
                }
            }
        }

        Ok(None)
    }

    /// Resolves a query using the configured upstream strategy.
    ///
    /// # Parameters
    ///
    /// - `query`: Normalized query to resolve.
    ///
    /// # Returns
    ///
    /// Answer produced by system, recursive, or forwarder resolution.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected upstream path fails.
    async fn resolve_upstream(&self, query: QueryRequest) -> Result<Answer> {
        match self.config.config.resolver.mode {
            ResolutionMode::Stub if self.config.config.upstreams.is_empty() => {
                self.resolve_system(query).await
            }
            ResolutionMode::Recursive if self.config.config.upstreams.is_empty() => {
                self.resolve_recursive(query).await
            }
            _ => self.resolve_forwarder(query).await,
        }
    }

    #[cfg(feature = "recursive")]
    /// Resolves a query using the recursive resolver feature.
    ///
    /// # Parameters
    ///
    /// - `query`: Query to resolve recursively.
    ///
    /// # Returns
    ///
    /// Recursive DNS answer.
    ///
    /// # Errors
    ///
    /// Returns an error when recursive resolution fails.
    async fn resolve_recursive(&self, query: QueryRequest) -> Result<Answer> {
        match crate::core::recursive::resolve_recursive(
            &query,
            self.config.config.resolver.timeout_ms,
            &self.config.config.resolver.root_hints,
        )
        .await?
        {
            crate::core::recursive::RecursiveResult::Answer(answer) => Ok(answer),
            crate::core::recursive::RecursiveResult::Negative { error, nsec_proofs } => {
                self.cache.put_nsec_proofs(nsec_proofs).await;
                Err(error)
            }
        }
    }

    #[cfg(not(feature = "recursive"))]
    /// Reports that recursive resolution is unavailable without the feature.
    ///
    /// # Parameters
    ///
    /// - `_query`: Query that would require recursive resolution.
    ///
    /// # Errors
    ///
    /// Always returns an unsupported-feature error.
    async fn resolve_recursive(&self, _query: QueryRequest) -> Result<Answer> {
        Err(DnsError::UnsupportedFeature(
            "recursive resolver requires the `recursive` feature".to_owned(),
        ))
    }

    /// Resolves a query through the system resolver.
    ///
    /// # Parameters
    ///
    /// - `query`: Query to resolve through the operating system.
    ///
    /// # Returns
    ///
    /// Answer synthesized from system resolver addresses.
    ///
    /// # Errors
    ///
    /// Returns an error when the system resolver fails or the record type is
    /// unsupported.
    async fn resolve_system(&self, query: QueryRequest) -> Result<Answer> {
        match query.record_type {
            RecordType::A | RecordType::AAAA => {
                let addrs = timeout(
                    Duration::from_millis(self.config.config.resolver.timeout_ms),
                    tokio::net::lookup_host((query.name.as_str(), 0)),
                )
                .await
                .map_err(|_| DnsError::Timeout)?
                .map_err(|error| DnsError::UpstreamUnavailable(error.to_string()))?;

                let records = addrs
                    .filter_map(|addr| match (query.record_type, addr.ip()) {
                        (RecordType::A, IpAddr::V4(address)) => Some(Record {
                            name: query.name.clone(),
                            ttl: 60,
                            data: RecordData::A { address },
                        }),
                        (RecordType::AAAA, IpAddr::V6(address)) => Some(Record {
                            name: query.name.clone(),
                            ttl: 60,
                            data: RecordData::AAAA { address },
                        }),
                        _ => None,
                    })
                    .collect::<Vec<_>>();

                if records.is_empty() {
                    return Err(DnsError::NoData {
                        name: query.name,
                        record_type: query.record_type,
                    });
                }

                Ok(answer_from_records(&query, records, None).with_upstream("system"))
            }
            other => Err(DnsError::UnsupportedFeature(format!(
                "system stub currently supports A/AAAA only, got {other:?}"
            ))),
        }
    }

    /// Resolves a query by trying configured forwarding upstreams.
    ///
    /// # Parameters
    ///
    /// - `query`: Query to forward.
    ///
    /// # Returns
    ///
    /// First successful upstream answer.
    ///
    /// # Errors
    ///
    /// Returns the last relevant upstream error when every upstream fails.
    async fn resolve_forwarder(&self, query: QueryRequest) -> Result<Answer> {
        let mut last_error = None;
        for upstream in &self.config.config.upstreams {
            match self.resolve_with_upstream(&query, upstream).await {
                Ok(answer) => return Ok(answer),
                Err(DnsError::Nxdomain(_))
                    if self.config.config.resolver.nxdomain_strategy
                        == crate::core::NxdomainStrategy::FallbackOnNxdomain =>
                {
                    last_error = Some(DnsError::Nxdomain(query.name.clone()));
                }
                Err(error) => {
                    self.stats.write().await.upstream_errors += 1;
                    warn!(%error, name = %query.name, "upstream resolution failed");
                    last_error = Some(error);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            DnsError::InvalidConfig("no upstream configured for forwarder mode".to_owned())
        }))
    }

    /// Resolves a query with one specific upstream.
    ///
    /// # Parameters
    ///
    /// - `query`: Query to resolve.
    /// - `upstream`: Upstream configuration to use.
    ///
    /// # Returns
    ///
    /// Answer returned by the upstream.
    ///
    /// # Errors
    ///
    /// Returns an error when the upstream transport is unsupported or the
    /// upstream exchange fails.
    async fn resolve_with_upstream(
        &self,
        query: &QueryRequest,
        upstream: &Upstream,
    ) -> Result<Answer> {
        #[cfg(feature = "dnssec")]
        if self.config.config.security.dnssec == DnssecState::Strict {
            if hickory_dnssec_transport_supported(upstream) {
                return crate::core::hickory::resolve_with_hickory(
                    query,
                    upstream,
                    true,
                    self.config.config.resolver.timeout_ms,
                )
                .await;
            }
            return Err(DnsError::UnsupportedFeature(format!(
                "DNSSEC strict validation is not implemented for {:?}",
                upstream.transport
            )));
        }

        match (&upstream.transport, &upstream.endpoint) {
            (Transport::Udp, UpstreamEndpoint::Socket(addr)) => {
                resolve_udp(
                    query,
                    upstream,
                    *addr,
                    self.config.config.resolver.timeout_ms,
                )
                .await
            }
            (Transport::Tcp, UpstreamEndpoint::Socket(addr)) => {
                resolve_tcp(
                    query,
                    upstream,
                    *addr,
                    self.config.config.resolver.timeout_ms,
                )
                .await
            }
            #[cfg(feature = "doh")]
            (Transport::Doh, UpstreamEndpoint::Url(url)) => {
                crate::core::transport::resolve_doh(
                    query,
                    upstream,
                    url,
                    self.config.config.resolver.timeout_ms,
                )
                .await
            }
            #[cfg(feature = "dot")]
            (Transport::Dot, UpstreamEndpoint::HostPort { host, port }) => {
                crate::core::transport::resolve_dot(
                    query,
                    upstream,
                    host,
                    *port,
                    self.config.config.resolver.timeout_ms,
                )
                .await
            }
            #[cfg(feature = "doq")]
            (Transport::Doq, UpstreamEndpoint::HostPort { .. }) => {
                crate::core::hickory::resolve_with_hickory(
                    query,
                    upstream,
                    self.config.config.security.dnssec != DnssecState::Off,
                    self.config.config.resolver.timeout_ms,
                )
                .await
            }
            (Transport::System, _) => self.resolve_system(query.clone()).await,
            (Transport::Dot, _) if !cfg!(feature = "dot") => Err(DnsError::UnsupportedFeature(
                "DoT support requires the `dot` feature".to_owned(),
            )),
            (Transport::Doh, _) if !cfg!(feature = "doh") => Err(DnsError::UnsupportedFeature(
                "DoH support requires the `doh` feature".to_owned(),
            )),
            (Transport::Doq, _) if !cfg!(feature = "doq") => Err(DnsError::UnsupportedFeature(
                "DoQ support requires the `doq` feature and a host_port endpoint".to_owned(),
            )),
            (Transport::Odoh, _) if !cfg!(feature = "odoh") => Err(DnsError::UnsupportedFeature(
                "ODoH support requires the `odoh` feature".to_owned(),
            )),
            (transport, _) => Err(DnsError::UnsupportedFeature(format!(
                "transport {transport:?} is declared but not implemented in this slice"
            ))),
        }
    }
}

/// Returns whether the Hickory DNSSEC path supports an upstream transport.
///
/// # Parameters
///
/// - `upstream`: Upstream considered for DNSSEC validation.
///
/// # Returns
///
/// `true` when the transport can be resolved by the Hickory path used here.
#[cfg(feature = "dnssec")]
fn hickory_dnssec_transport_supported(upstream: &Upstream) -> bool {
    matches!(
        (&upstream.transport, &upstream.endpoint),
        (Transport::Udp | Transport::Tcp, UpstreamEndpoint::Socket(_))
            | (
                Transport::Dot | Transport::Doq,
                UpstreamEndpoint::HostPort { .. }
            )
    )
}

/// Builds an answer from already-selected records.
///
/// # Parameters
///
/// - `query`: Query associated with the records.
/// - `records`: Records to include in the answer.
/// - `policy`: Optional policy metadata.
///
/// # Returns
///
/// Answer with TTL derived from the included records.
fn answer_from_records(
    query: &QueryRequest,
    records: Vec<Record>,
    policy: Option<PolicyDecision>,
) -> Answer {
    let ttl = records.iter().map(|record| record.ttl).min();
    Answer {
        query: query.clone(),
        records,
        metadata: AnswerMetadata {
            ttl,
            cname_chain: Vec::new(),
            upstream: None,
            cache: CacheState::Disabled,
            dnssec: DnssecState::Off,
            policy,
        },
    }
}

/// Helper trait for attaching upstream metadata to answers.
trait WithUpstream {
    /// Attaches upstream metadata to an answer.
    ///
    /// # Parameters
    ///
    /// - `upstream`: Upstream label to record.
    ///
    /// # Returns
    ///
    /// Updated answer.
    fn with_upstream(self, upstream: &str) -> Self;
}

impl WithUpstream for Answer {
    /// Attaches upstream metadata to an answer.
    ///
    /// # Parameters
    ///
    /// - `upstream`: Upstream label to record.
    ///
    /// # Returns
    ///
    /// Updated answer.
    fn with_upstream(mut self, upstream: &str) -> Self {
        self.metadata.upstream = Some(upstream.to_owned());
        self
    }
}

/// Builds the answer or error for a block action.
///
/// # Parameters
///
/// - `query`: Query being blocked.
/// - `action`: Configured block action.
/// - `rule_name`: Optional name of the matching rule.
/// - `source`: Policy source that produced the block.
///
/// # Returns
///
/// Optional answer for block modes that synthesize DNS data.
///
/// # Errors
///
/// Returns DNS errors for block modes represented as resolver errors.
fn block_answer(
    query: &QueryRequest,
    action: &BlockAction,
    rule_name: Option<String>,
    source: PolicySource,
) -> Result<Option<Answer>> {
    let decision = Some(PolicyDecision {
        source,
        rule_name,
        action: "block".to_owned(),
    });
    match action {
        BlockAction::Nxdomain => Err(DnsError::Nxdomain(query.name.clone())),
        BlockAction::NoData | BlockAction::Empty => {
            Ok(Some(answer_from_records(query, Vec::new(), decision)))
        }
        BlockAction::Refused => Err(DnsError::Blocked(query.name.clone())),
        BlockAction::Address { ipv4, ipv6 } => {
            let mut records = Vec::new();
            if query.record_type == RecordType::A {
                if let Some(address) = ipv4 {
                    records.push(Record {
                        name: query.name.clone(),
                        ttl: 60,
                        data: RecordData::A { address: *address },
                    });
                }
            }
            if query.record_type == RecordType::AAAA {
                if let Some(address) = ipv6 {
                    records.push(Record {
                        name: query.name.clone(),
                        ttl: 60,
                        data: RecordData::AAAA { address: *address },
                    });
                }
            }
            Ok(Some(answer_from_records(query, records, decision)))
        }
    }
}
