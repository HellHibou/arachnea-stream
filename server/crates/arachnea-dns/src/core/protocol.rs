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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct QueryRequest {
    /// Domain name, normalized to lower case without a trailing dot.
    pub name: String,
    /// Requested record type.
    pub record_type: RecordType,
    /// DNS class. Version 1 supports only IN.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum DnsClass {
    /// Internet class.
    IN,
}

/// Public DNS record types supported by version 1 APIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum RecordType {
    /// IPv4 address.
    A,
    /// IPv6 address.
    AAAA,
    /// Canonical name.
    CNAME,
    /// Mail exchanger.
    MX,
    /// Text record.
    TXT,
    /// Service locator.
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RecordData {
    /// IPv4 address payload.
    A { address: Ipv4Addr },
    /// IPv6 address payload.
    AAAA { address: Ipv6Addr },
    /// Canonical name target.
    CNAME { name: String },
    /// Mail exchanger payload.
    MX { preference: u16, exchange: String },
    /// Text payload.
    TXT { value: String },
    /// Service locator payload.
    SRV {
        priority: u16,
        weight: u16,
        port: u16,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// Owner name.
    pub name: String,
    /// Time to live in seconds.
    pub ttl: u32,
    /// Typed record payload.
    pub data: RecordData,
}

/// Complete answer for a DNS query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Answer {
    /// Original normalized query.
    pub query: QueryRequest,
    /// Records returned for the query.
    pub records: Vec<Record>,
    /// Cache, upstream, DNSSEC and policy metadata.
    pub metadata: AnswerMetadata,
}

/// Metadata attached to a DNS answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerMetadata {
    /// Minimum TTL across returned records.
    pub ttl: Option<u32>,
    /// CNAME targets followed or observed in the answer.
    pub cname_chain: Vec<String>,
    /// Name of the upstream that produced the answer.
    pub upstream: Option<String>,
    /// Cache state for the answer.
    pub cache: crate::core::CacheState,
    /// DNSSEC state associated with the answer.
    pub dnssec: crate::core::DnssecState,
    /// Winning policy decision, when any.
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
