//! WebVTT document parsing, validation and serialization.
//!
//! The parser keeps cue identifiers, raw timing lines (including settings),
//! payload lines, and `NOTE`, `STYLE` and `REGION` blocks. Normalization only
//! changes line endings and the leading byte-order mark; timestamps and cue
//! settings are preserved verbatim. Translation never rewrites timing data.

use super::error::SubtitleTranslationError;

/// Parsed WebVTT document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebVttDocument {
    /// Signature line and optional header metadata.
    pub header: WebVttHeader,
    /// Document blocks in source order.
    pub blocks: Vec<WebVttBlock>,
}

/// WebVTT signature and header metadata lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebVttHeader {
    /// Signature line, from `WEBVTT` up to the end of the line.
    pub signature: String,
    /// Optional header metadata lines, kept verbatim.
    pub metadata: Vec<String>,
}

/// One WebVTT block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebVttBlock {
    /// A subtitle cue.
    Cue(WebVttCue),
    /// A `NOTE` comment block, including its first line.
    Note(Vec<String>),
    /// A `STYLE` block, including its first line.
    Style(Vec<String>),
    /// A `REGION` block, including its first line.
    Region(Vec<String>),
}

/// One WebVTT cue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebVttCue {
    /// Optional cue identifier.
    pub identifier: Option<String>,
    /// Raw timing line, including cue settings.
    pub timing: String,
    /// Raw cue payload lines.
    pub payload: Vec<String>,
}

impl WebVttDocument {
    /// Parses and validates a complete WebVTT document.
    ///
    /// The input must be valid UTF-8. A leading byte-order mark is accepted,
    /// and CRLF/CR line endings are normalized to LF. The signature line, cue
    /// identifiers, timing lines, payload lines, `NOTE`, `STYLE`, `REGION` and
    /// header metadata are preserved.
    ///
    /// # Arguments
    ///
    /// * `bytes` - Complete source document bytes.
    ///
    /// # Errors
    ///
    /// Returns [`SubtitleTranslationError::InvalidDocument`] for invalid UTF-8,
    /// a missing or malformed signature, a malformed block, an invalid
    /// timestamp or an end time not strictly after its start time.
    pub fn parse(bytes: &[u8]) -> Result<Self, SubtitleTranslationError> {
        let text = std::str::from_utf8(bytes).map_err(|error| {
            SubtitleTranslationError::InvalidDocument {
                detail: format!("WebVTT document is not valid UTF-8: {error}"),
            }
        })?;
        let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");

        let mut lines = normalized.split('\n');
        let signature = lines
            .next()
            .ok_or_else(|| invalid_document("WebVTT document is empty"))?;
        if !is_signature(signature) {
            return Err(invalid_document(format!(
                "invalid WebVTT signature line {signature:?}"
            )));
        }

        let mut metadata = Vec::new();
        for line in lines.by_ref() {
            if line.is_empty() {
                break;
            }
            if line.contains("-->") {
                return Err(invalid_document(
                    "WebVTT header metadata must not contain '-->'",
                ));
            }
            metadata.push(line.to_string());
        }

        let mut blocks = Vec::new();
        let mut current: Vec<String> = Vec::new();
        for line in lines {
            if line.is_empty() {
                if !current.is_empty() {
                    blocks.push(parse_block(&current)?);
                    current.clear();
                }
            } else {
                current.push(line.to_string());
            }
        }
        if !current.is_empty() {
            blocks.push(parse_block(&current)?);
        }

        Ok(Self {
            header: WebVttHeader {
                signature: signature.to_string(),
                metadata,
            },
            blocks,
        })
    }

    /// Returns the number of subtitle cues in the document.
    pub fn cue_count(&self) -> u64 {
        self.blocks
            .iter()
            .filter(|block| matches!(block, WebVttBlock::Cue(_)))
            .count() as u64
    }

    /// Serializes the document as normalized UTF-8 WebVTT text.
    ///
    /// Blocks are separated by exactly one blank line, line endings are LF and
    /// the output ends with a newline. Header metadata and raw block lines are
    /// emitted unchanged.
    pub fn to_webvtt(&self) -> String {
        let mut output = String::new();
        output.push_str(&self.header.signature);
        output.push('\n');
        for line in &self.header.metadata {
            output.push_str(line);
            output.push('\n');
        }
        output.push('\n');

        for (index, block) in self.blocks.iter().enumerate() {
            if index > 0 {
                output.push('\n');
            }
            for line in block_lines(block) {
                output.push_str(line);
                output.push('\n');
            }
        }
        output
    }

    /// Serializes the document as UTF-8 bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.to_webvtt().into_bytes()
    }
}

/// Returns whether a line is a valid WebVTT signature line.
///
/// # Arguments
///
/// * `line` - First document line without its line ending.
fn is_signature(line: &str) -> bool {
    line == "WEBVTT" || line.starts_with("WEBVTT ") || line.starts_with("WEBVTT\t")
}

/// Parses one non-empty block.
///
/// # Arguments
///
/// * `lines` - Block lines, without surrounding blank separators.
///
/// # Errors
///
/// Returns [`SubtitleTranslationError::InvalidDocument`] for an unknown block
/// or an invalid cue structure.
fn parse_block(lines: &[String]) -> Result<WebVttBlock, SubtitleTranslationError> {
    let first = &lines[0];
    if is_note_line(first) {
        return Ok(WebVttBlock::Note(lines.to_vec()));
    }
    if first == "STYLE" {
        return Ok(WebVttBlock::Style(lines.to_vec()));
    }
    if first == "REGION" {
        return Ok(WebVttBlock::Region(lines.to_vec()));
    }
    parse_cue(lines).map(WebVttBlock::Cue)
}

/// Returns whether a line starts a `NOTE` block.
///
/// # Arguments
///
/// * `line` - First line of a candidate block.
fn is_note_line(line: &str) -> bool {
    line == "NOTE" || line.starts_with("NOTE ") || line.starts_with("NOTE\t")
}

/// Parses one cue block.
///
/// # Arguments
///
/// * `lines` - Cue block lines.
///
/// # Errors
///
/// Returns [`SubtitleTranslationError::InvalidDocument`] when the timing line
/// is missing, malformed or temporally invalid.
fn parse_cue(lines: &[String]) -> Result<WebVttCue, SubtitleTranslationError> {
    let (identifier, timing_index) = if lines[0].contains("-->") {
        (None, 0)
    } else {
        if lines.len() < 2 || !lines[1].contains("-->") {
            return Err(invalid_document(
                "WebVTT block is neither a cue, NOTE, STYLE nor REGION block",
            ));
        }
        (Some(lines[0].clone()), 1)
    };

    let timing = lines[timing_index].clone();
    validate_timing(&timing)?;
    let payload = lines[timing_index + 1..].to_vec();
    Ok(WebVttCue {
        identifier,
        timing,
        payload,
    })
}

/// Validates a cue timing line and its temporal ordering.
///
/// # Arguments
///
/// * `timing` - Raw timing line, including optional cue settings.
///
/// # Errors
///
/// Returns [`SubtitleTranslationError::InvalidDocument`] when the timing line
/// cannot be parsed or the end time is not strictly after the start time.
fn validate_timing(timing: &str) -> Result<(), SubtitleTranslationError> {
    let (start_part, end_part) = timing
        .split_once("-->")
        .ok_or_else(|| invalid_document("WebVTT cue timing line has no '-->' separator"))?;
    let start_token = start_part.trim();
    let end_rest = end_part.trim_start();
    let end_token = end_rest
        .split_whitespace()
        .next()
        .ok_or_else(|| invalid_document("WebVTT cue timing line has no end timestamp"))?;

    let start = parse_timestamp(start_token).ok_or_else(|| {
        invalid_document(format!("invalid WebVTT start timestamp {start_token:?}"))
    })?;
    let end = parse_timestamp(end_token)
        .ok_or_else(|| invalid_document(format!("invalid WebVTT end timestamp {end_token:?}")))?;
    if end <= start {
        return Err(invalid_document(
            "WebVTT cue end timestamp must be strictly after its start timestamp",
        ));
    }
    Ok(())
}

/// Parses one WebVTT timestamp into milliseconds.
///
/// Accepts `mm:ss.mmm` and `hh:mm:ss.mmm` with exactly three millisecond
/// digits. Hours may have two or more digits. Returns `None` for any other
/// shape, out-of-range component or arithmetic overflow.
///
/// # Arguments
///
/// * `token` - Timestamp without surrounding whitespace.
pub(super) fn parse_timestamp(token: &str) -> Option<u64> {
    let (clock, millis_text) = token.rsplit_once('.')?;
    if millis_text.len() != 3 || !millis_text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let millis = millis_text.parse::<u64>().ok()?;

    let parts: Vec<&str> = clock.split(':').collect();
    let (hours, minutes, seconds) = match parts.as_slice() {
        [minutes, seconds] => (0u64, parse_two_digits(minutes)?, parse_two_digits(seconds)?),
        [hours, minutes, seconds] => (
            parse_digits(hours, 2)?,
            parse_two_digits(minutes)?,
            parse_two_digits(seconds)?,
        ),
        _ => return None,
    };
    if minutes >= 60 || seconds >= 60 {
        return None;
    }

    let total_seconds = hours
        .checked_mul(3_600)?
        .checked_add(minutes.checked_mul(60)?)?
        .checked_add(seconds)?;
    total_seconds.checked_mul(1_000)?.checked_add(millis)
}

/// Parses a component made of exactly two ASCII digits.
///
/// # Arguments
///
/// * `value` - Component text.
fn parse_two_digits(value: &str) -> Option<u64> {
    if value.len() != 2 {
        return None;
    }
    parse_digits(value, 2)
}

/// Parses an ASCII-digit component with a minimum length.
///
/// # Arguments
///
/// * `value` - Component text.
/// * `minimum_length` - Minimum accepted digit count.
fn parse_digits(value: &str, minimum_length: usize) -> Option<u64> {
    if value.len() < minimum_length || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse::<u64>().ok()
}

/// Returns the serialized lines of one block.
///
/// # Arguments
///
/// * `block` - Block to serialize.
fn block_lines(block: &WebVttBlock) -> Vec<&str> {
    match block {
        WebVttBlock::Cue(cue) => {
            let mut lines = Vec::with_capacity(cue.payload.len() + 2);
            if let Some(identifier) = &cue.identifier {
                lines.push(identifier.as_str());
            }
            lines.push(cue.timing.as_str());
            lines.extend(cue.payload.iter().map(String::as_str));
            lines
        }
        WebVttBlock::Note(lines) | WebVttBlock::Style(lines) | WebVttBlock::Region(lines) => {
            lines.iter().map(String::as_str).collect()
        }
    }
}

/// Builds a contextualized invalid-document error.
///
/// # Arguments
///
/// * `detail` - Human-readable cause.
fn invalid_document(detail: impl Into<String>) -> SubtitleTranslationError {
    SubtitleTranslationError::InvalidDocument {
        detail: detail.into(),
    }
}
