use crate::core::{DnsError, Result};
use hickory_proto::rr::{
    rdata::{A, AAAA, CNAME, MX, SRV, TXT},
    Name, RData, Record as HickoryRecord, RecordType as HickoryRecordType,
};
use serde::{Deserialize, Serialize};
use std::{
    net::{Ipv4Addr, Ipv6Addr},
    str::FromStr,
};

/// A normalized DNS query used by the public core API.
/// 
/// This struct represents a DNS query that has been normalized for consistent
/// processing. The domain name is converted to lowercase and any trailing dots
/// are removed to ensure consistent comparison and caching.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct QueryRequest {
    /// Domain name, normalized to lower case without a trailing dot.
    /// 
    /// This field contains the fully qualified domain name in a normalized form
    /// suitable for DNS lookups and cache key generation.
    pub name: String,
    /// Requested record type.
    /// 
    /// Specifies the type of DNS record being requested (e.g., A, AAAA, MX).
    pub record_type: RecordType,
    /// DNS class. Version 1 supports only IN.
    /// 
    /// Currently only the Internet (IN) class is supported, as specified in
    /// the DNS protocol standards.
    pub class: DnsClass,
}

impl QueryRequest {
    /// Creates a query for the IN class and normalizes the name.
    ///
    /// # Parameters
    ///
    /// - `name`: DNS name to query.
    /// - `record_type`: DNS record type to request.
    ///
    /// # Returns
    ///
    /// Query request with normalized name and IN class.
    pub fn new(name: impl Into<String>, record_type: RecordType) -> Self {
        Self {
            name: normalize_name(&name.into()),
            record_type,
            class: DnsClass::IN,
        }
    }
}

/// DNS class supported by the resolver.
/// 
/// This enum represents the DNS class field as defined in RFC 1035.
/// Currently only the Internet (IN) class is supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum DnsClass {
    /// Internet class.
    /// 
    /// The IN class is the standard DNS class used for Internet resources.
    IN,
}

/// Public DNS record types supported by version 1 APIs.
/// 
/// This enum represents the standard DNS record types that are supported
/// by the resolver. Each variant corresponds to a specific type of DNS
/// resource record as defined in RFC 1035 and related standards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum RecordType {
    /// IPv4 address.
    /// 
    /// A records map domain names to IPv4 addresses.
    A,
    /// IPv6 address.
    /// 
    /// AAAA records map domain names to IPv6 addresses.
    AAAA,
    /// Canonical name.
    /// 
    /// CNAME records specify that a domain name is an alias for another
    /// domain name (the canonical name).
    CNAME,
    /// Mail exchanger.
    /// 
    /// MX records specify mail exchange servers for a domain, used for
    /// email routing.
    MX,
    /// Text record.
    /// 
    /// TXT records contain arbitrary text data and are often used for
    /// domain verification and configuration.
    TXT,
    /// Service locator.
    /// 
    /// SRV records specify the location of services (such as LDAP or SIP)
    /// within a domain.
    SRV,
}

impl RecordType {
    /// Converts this record type to Hickory's record type.
    ///
    /// # Returns
    ///
    /// Equivalent Hickory record type.
    pub(crate) fn to_hickory(self) -> HickoryRecordType {
        match self {
            Self::A => HickoryRecordType::A,
            Self::AAAA => HickoryRecordType::AAAA,
            Self::CNAME => HickoryRecordType::CNAME,
            Self::MX => HickoryRecordType::MX,
            Self::TXT => HickoryRecordType::TXT,
            Self::SRV => HickoryRecordType::SRV,
        }
    }
}

impl TryFrom<HickoryRecordType> for RecordType {
    /// Error returned when a Hickory record type is unsupported.
    type Error = DnsError;

    /// Converts a Hickory record type into this crate's record type.
    ///
    /// # Parameters
    ///
    /// - `value`: Hickory record type to convert.
    ///
    /// # Returns
    ///
    /// Equivalent record type.
    ///
    /// # Errors
    ///
    /// Returns an error when the record type is unsupported.
    fn try_from(value: HickoryRecordType) -> Result<Self> {
        match value {
            HickoryRecordType::A => Ok(Self::A),
            HickoryRecordType::AAAA => Ok(Self::AAAA),
            HickoryRecordType::CNAME => Ok(Self::CNAME),
            HickoryRecordType::MX => Ok(Self::MX),
            HickoryRecordType::TXT => Ok(Self::TXT),
            HickoryRecordType::SRV => Ok(Self::SRV),
            other => Err(DnsError::UnsupportedFeature(format!(
                "record type {other:?}"
            ))),
        }
    }
}

impl FromStr for RecordType {
    /// Error returned when parsing a record type fails.
    type Err = DnsError;

    /// Parses a DNS record type from text.
    ///
    /// # Parameters
    ///
    /// - `value`: Record type text such as `A` or `AAAA`.
    ///
    /// # Returns
    ///
    /// Parsed record type.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` is not supported.
    fn from_str(value: &str) -> Result<Self> {
        match value.to_ascii_uppercase().as_str() {
            "A" => Ok(Self::A),
            "AAAA" => Ok(Self::AAAA),
            "CNAME" => Ok(Self::CNAME),
            "MX" => Ok(Self::MX),
            "TXT" => Ok(Self::TXT),
            "SRV" => Ok(Self::SRV),
            other => Err(DnsError::UnsupportedFeature(format!("record type {other}"))),
        }
    }
}

/// Data stored in a DNS resource record.
/// 
/// This enum represents the payload data for different types of DNS
/// resource records. Each variant corresponds to a specific record type
/// and contains the appropriate data fields for that type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RecordData {
    /// IPv4 address payload.
    ///
    /// Contains a single IPv4 address that the domain name resolves to.
    A {
        /// IPv4 address.
        address: Ipv4Addr,
    },
    /// IPv6 address payload.
    ///
    /// Contains a single IPv6 address that the domain name resolves to.
    AAAA {
        /// IPv6 address.
        address: Ipv6Addr,
    },
    /// Canonical name target.
    ///
    /// Contains the canonical domain name that this domain is an alias for.
    CNAME {
        /// Canonical domain name.
        name: String,
    },
    /// Mail exchanger payload.
    ///
    /// Contains the preference value (lower is better) and the domain name
    /// of the mail exchange server.
    MX {
        /// Preference value (lower values are preferred).
        preference: u16,
        /// Mail exchange server domain name.
        exchange: String,
    },
    /// Text payload.
    ///
    /// Contains arbitrary text data associated with the domain name.
    TXT {
        /// Text content.
        value: String,
    },
    /// Service locator payload.
    ///
    /// Contains priority, weight, port, and target domain for service
    /// discovery. Lower priority values are preferred. Weight is used
    /// for load balancing among services with the same priority.
    SRV {
        /// Priority value (lower values are preferred).
        priority: u16,
        /// Load-balancing weight for same-priority targets.
        weight: u16,
        /// Service port.
        port: u16,
        /// Target domain providing the service.
        target: String,
    },
}

impl RecordData {
    /// Returns the DNS type represented by the payload.
    pub fn record_type(&self) -> RecordType {
        match self {
            Self::A { .. } => RecordType::A,
            Self::AAAA { .. } => RecordType::AAAA,
            Self::CNAME { .. } => RecordType::CNAME,
            Self::MX { .. } => RecordType::MX,
            Self::TXT { .. } => RecordType::TXT,
            Self::SRV { .. } => RecordType::SRV,
        }
    }
}

/// Public resource record returned by the resolver.
/// 
/// This struct represents a single DNS resource record as returned by
/// the DNS resolver. It contains the domain name, time-to-live, and
/// the record-specific data payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// Owner name.
    /// 
    /// The domain name to which this record applies.
    pub name: String,
    /// Time to live in seconds.
    /// 
    /// Specifies how long the record can be cached before it should be
    /// considered stale and refreshed from an authoritative source.
    pub ttl: u32,
    /// Typed record payload.
    /// 
    /// Contains the record-type-specific data for this resource record.
    pub data: RecordData,
}

/// Complete answer for a DNS query.
/// 
/// This struct represents the complete response to a DNS query, including
/// the original query information, the resource records that answer the
/// query, and metadata about how the answer was obtained.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Answer {
    /// Original normalized query.
    /// 
    /// The query that produced this answer, in normalized form.
    pub query: QueryRequest,
    /// Records returned for the query.
    /// 
    /// The resource records that satisfy the DNS query. This may include
    /// records of the requested type as well as CNAME records that form
    /// a chain of aliases.
    pub records: Vec<Record>,
    /// Cache, upstream, DNSSEC and policy metadata.
    /// 
    /// Additional information about how this answer was obtained, including
    /// cache status, upstream server used, DNSSEC validation status, and
    /// any policy decisions that affected the answer.
    pub metadata: AnswerMetadata,
}

/// Metadata attached to a DNS answer.
/// 
/// This struct contains additional information about a DNS answer that
/// describes how it was obtained and processed, including cache status,
/// upstream server information, and security validation results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerMetadata {
    /// Minimum TTL across returned records.
    /// 
    /// The smallest TTL value among all records in the answer, which
    /// determines how long the entire answer can be cached.
    pub ttl: Option<u32>,
    /// CNAME targets followed or observed in the answer.
    /// 
    /// A list of domain names that form a chain of CNAME aliases that
    /// were followed to resolve the original query.
    pub cname_chain: Vec<String>,
    /// Name of the upstream that produced the answer.
    /// 
    /// The name of the upstream DNS server that provided this answer,
    /// if the answer came from an upstream server.
    pub upstream: Option<String>,
    /// Cache state for the answer.
    /// 
    /// Indicates whether this answer came from cache, and if so,
    /// whether it was fresh or stale.
    pub cache: crate::core::CacheState,
    /// DNSSEC state associated with the answer.
    /// 
    /// Indicates the DNSSEC validation status of this answer.
    pub dnssec: crate::core::DnssecState,
    /// Winning policy decision, when any.
    /// 
    /// If a policy (such as a blocklist or routing rule) affected this
    /// answer, this field contains information about which policy was
    /// applied and what action it took.
    pub policy: Option<crate::core::PolicyDecision>,
}

/// Converts a core record into Hickory's protocol record.
///
/// # Parameters
///
/// - `record`: Core record to convert.
///
/// # Returns
///
/// Hickory protocol record.
///
/// # Errors
///
/// Returns an error when record names or payloads cannot be encoded.
pub fn record_to_hickory(record: &Record) -> Result<HickoryRecord> {
    let name = Name::from_ascii(format!("{}.", normalize_name(&record.name)))
        .map_err(|error| DnsError::Protocol(error.to_string()))?;
    let rdata = match &record.data {
        RecordData::A { address } => RData::A(A(*address)),
        RecordData::AAAA { address } => RData::AAAA(AAAA(*address)),
        RecordData::CNAME { name } => RData::CNAME(CNAME(
            Name::from_ascii(format!("{}.", normalize_name(name)))
                .map_err(|error| DnsError::Protocol(error.to_string()))?,
        )),
        RecordData::MX {
            preference,
            exchange,
        } => RData::MX(MX::new(
            *preference,
            Name::from_ascii(format!("{}.", normalize_name(exchange)))
                .map_err(|error| DnsError::Protocol(error.to_string()))?,
        )),
        RecordData::TXT { value } => RData::TXT(TXT::new(vec![value.clone()])),
        RecordData::SRV {
            priority,
            weight,
            port,
            target,
        } => RData::SRV(SRV::new(
            *priority,
            *weight,
            *port,
            Name::from_ascii(format!("{}.", normalize_name(target)))
                .map_err(|error| DnsError::Protocol(error.to_string()))?,
        )),
    };
    Ok(HickoryRecord::from_rdata(name, record.ttl, rdata))
}

/// Converts several core records into Hickory protocol records.
///
/// # Parameters
///
/// - `records`: Core records to convert.
///
/// # Returns
///
/// Hickory protocol records.
///
/// # Errors
///
/// Returns an error when any record cannot be encoded.
pub fn records_to_hickory(records: &[Record]) -> Result<Vec<HickoryRecord>> {
    records.iter().map(record_to_hickory).collect()
}

/// Converts a Hickory record into this crate's record representation.
///
/// # Parameters
///
/// - `record`: Hickory record to convert.
///
/// # Returns
///
/// Converted record, or `None` when the record data is intentionally ignored.
///
/// # Errors
///
/// Returns an error when the record data is unsupported or malformed.
pub(crate) fn record_from_hickory(record: &HickoryRecord) -> Result<Option<Record>> {
    let name = normalize_name(&record.name().to_ascii());
    let ttl = record.ttl();
    let data = match record.data() {
        RData::A(value) => RecordData::A { address: value.0 },
        RData::AAAA(value) => RecordData::AAAA { address: value.0 },
        RData::CNAME(value) => RecordData::CNAME {
            name: normalize_name(&value.0.to_ascii()),
        },
        RData::MX(value) => RecordData::MX {
            preference: value.preference(),
            exchange: normalize_name(&value.exchange().to_ascii()),
        },
        RData::TXT(value) => RecordData::TXT {
            value: value
                .txt_data()
                .iter()
                .map(|part| String::from_utf8_lossy(part))
                .collect::<Vec<_>>()
                .join(""),
        },
        RData::SRV(value) => RecordData::SRV {
            priority: value.priority(),
            weight: value.weight(),
            port: value.port(),
            target: normalize_name(&value.target().to_ascii()),
        },
        _ => return Ok(None),
    };

    Ok(Some(Record { name, ttl, data }))
}

/// Normalizes a DNS name for comparisons and cache keys.
///
/// # Parameters
///
/// - `value`: DNS name to normalize.
///
/// # Returns
///
/// Lowercase DNS name without a trailing dot.
pub fn normalize_name(value: &str) -> String {
    value.trim().trim_end_matches('.').to_ascii_lowercase()
}
