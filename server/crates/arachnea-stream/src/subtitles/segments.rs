//! Cue text extraction, markup protection and segment reconstruction.
//!
//! Only visible cue text is submitted to translation. Inline WebVTT tags and
//! internal timestamps are kept verbatim and act as hard segment boundaries, so
//! a provider can never alter or drop markup. Long text runs are split at
//! sentence, word, or last-resort Unicode character boundaries. The segment
//! table records which abstract block belongs to each part of each cue.

use super::error::SubtitleTranslationError;
use super::webvtt::{WebVttBlock, WebVttDocument};
use arachnea_translation::{
    plan_batches, BlockId, LocalTranslationLimits, TextBlock, TextTranslationCapability,
    TranslatedTexts, TranslationError,
};

/// Document whose cue payloads have been split into translatable segments.
#[derive(Debug, Clone)]
pub(crate) struct SegmentedDocument {
    /// Parsed source document.
    document: WebVttDocument,
    /// Planned parts for each cue, in document order.
    cues: Vec<PlannedCue>,
    /// Abstract translation blocks in deterministic order.
    blocks: Vec<TextBlock>,
}

impl SegmentedDocument {
    /// Returns the parsed source document.
    pub(crate) fn document(&self) -> &WebVttDocument {
        &self.document
    }

    /// Returns the abstract blocks submitted to translation.
    pub(crate) fn blocks(&self) -> &[TextBlock] {
        &self.blocks
    }

    /// Rebuilds a document from normalized translated blocks.
    ///
    /// Blocks must match the planned identifiers, count and order. Empty
    /// translated segments are rejected so no partial or silently missing cue
    /// text is published.
    ///
    /// # Arguments
    ///
    /// * `translated` - Normalized translation result.
    ///
    /// # Errors
    ///
    /// Returns [`SubtitleTranslationError::Translation`] with
    /// [`TranslationError::InvalidResponse`] when identifiers, cardinality or
    /// segment text do not match the plan.
    pub(crate) fn apply(
        &self,
        translated: &TranslatedTexts,
    ) -> Result<WebVttDocument, SubtitleTranslationError> {
        if translated.blocks.len() != self.blocks.len() {
            return Err(invalid_response(format!(
                "expected {} translated blocks, received {}",
                self.blocks.len(),
                translated.blocks.len()
            )));
        }

        let mut rendered = Vec::with_capacity(self.blocks.len());
        for (expected, actual) in self.blocks.iter().zip(&translated.blocks) {
            if expected.id != actual.id {
                return Err(invalid_response(format!(
                    "translated block order mismatch: expected {}, received {}",
                    expected.id.as_str(),
                    actual.id.as_str()
                )));
            }
            let text = sanitize_translation(&actual.text);
            if text.trim().is_empty() {
                return Err(invalid_response(format!(
                    "translated block {} is empty",
                    actual.id.as_str()
                )));
            }
            rendered.push(text);
        }

        let mut document = self.document.clone();
        for planned in &self.cues {
            let Some(WebVttBlock::Cue(cue)) = document.blocks.get_mut(planned.block_index) else {
                return Err(invalid_response(
                    "planned cue no longer matches the source document",
                ));
            };
            let payload = render_payload(planned, &rendered);
            cue.payload = payload
                .split('\n')
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect();
        }
        Ok(document)
    }
}

/// Splits a parsed WebVTT document into translatable blocks.
///
/// # Arguments
///
/// * `document` - Parsed source document.
/// * `capability` - Selected text translation capability.
/// * `local` - Local translation bounds applied during splitting.
///
/// # Errors
///
/// Returns [`SubtitleTranslationError::InvalidDocument`] for malformed cue
/// markup and propagates translation size or identifier errors.
pub(crate) fn segment(
    document: WebVttDocument,
    capability: &TextTranslationCapability,
    local: &LocalTranslationLimits,
) -> Result<SegmentedDocument, SubtitleTranslationError> {
    let mut cues = Vec::new();
    let mut blocks = Vec::new();

    for (block_index, block) in document.blocks.iter().enumerate() {
        let WebVttBlock::Cue(cue) = block else {
            continue;
        };
        let payload = cue.payload.join("\n");
        let raw_parts = tokenize(&payload)?;
        let mut planned_parts = Vec::new();
        let mut run_index = 0usize;

        for part in raw_parts {
            match part {
                RawPart::Raw(text) => planned_parts.push(PlannedPart::Raw(text)),
                RawPart::Text(text) => {
                    let prefix = format!("cue-{block_index}:run-{run_index}");
                    run_index += 1;
                    let pieces = split_text_run(&text, &prefix, capability, local)?;
                    let mut segments = Vec::with_capacity(pieces.len());
                    for piece in pieces {
                        let segment_index = blocks.len();
                        blocks.push(piece.block);
                        segments.push(PlannedSegment {
                            block_index: segment_index,
                            joiner_after: piece.joiner_after,
                        });
                    }
                    if !segments.is_empty() {
                        planned_parts.push(PlannedPart::Segments(segments));
                    }
                }
            }
        }

        cues.push(PlannedCue {
            block_index,
            parts: planned_parts,
        });
    }

    Ok(SegmentedDocument {
        document,
        cues,
        blocks,
    })
}

/// One cue's planned payload parts.
#[derive(Debug, Clone)]
struct PlannedCue {
    /// Index of the cue in the parsed document block list.
    block_index: usize,
    /// Ordered payload parts.
    parts: Vec<PlannedPart>,
}

/// One planned payload part.
#[derive(Debug, Clone)]
enum PlannedPart {
    /// Markup, internal timestamp or non-translatable whitespace kept verbatim.
    Raw(String),
    /// One or more translated segments.
    Segments(Vec<PlannedSegment>),
}

/// One translated segment and the original whitespace after it.
#[derive(Debug, Clone)]
struct PlannedSegment {
    /// Index of the block in the plan block list.
    block_index: usize,
    /// Original whitespace separating this segment from the next split part.
    joiner_after: String,
}

/// One tokenized cue text part.
#[derive(Debug, Clone)]
enum RawPart {
    /// Text that must be translated, with entities decoded.
    Text(String),
    /// Markup, internal timestamp or whitespace-only text kept verbatim.
    Raw(String),
}

/// One split text piece and its associated abstract block.
#[derive(Debug, Clone)]
struct TextPiece {
    /// Abstract block carrying the piece text.
    block: TextBlock,
    /// Original whitespace after the piece, reinserted during reconstruction.
    joiner_after: String,
}

/// Tokenizes a cue payload into translatable text and protected raw parts.
///
/// # Arguments
///
/// * `payload` - Cue payload with lines joined by `\n`.
///
/// # Errors
///
/// Returns [`SubtitleTranslationError::InvalidDocument`] when a `<` has no
/// matching `>`.
fn tokenize(payload: &str) -> Result<Vec<RawPart>, SubtitleTranslationError> {
    let mut parts = Vec::new();
    let mut cursor = 0usize;

    while let Some(relative_open) = payload[cursor..].find('<') {
        let open = cursor + relative_open;
        if open > cursor {
            push_text(&mut parts, &payload[cursor..open]);
        }
        let relative_close =
            payload[open..]
                .find('>')
                .ok_or_else(|| SubtitleTranslationError::InvalidDocument {
                    detail: "WebVTT cue text contains an unclosed '<'".to_string(),
                })?;
        let close = open + relative_close;
        parts.push(RawPart::Raw(payload[open..=close].to_string()));
        cursor = close + 1;
    }
    if cursor < payload.len() {
        push_text(&mut parts, &payload[cursor..]);
    }
    Ok(parts)
}

/// Pushes one text run, decoding entities and preserving whitespace-only runs.
///
/// # Arguments
///
/// * `parts` - Destination token list.
/// * `raw` - Raw text run between two protected parts.
fn push_text(parts: &mut Vec<RawPart>, raw: &str) {
    if raw.is_empty() {
        return;
    }
    let decoded = decode_entities(raw);
    if decoded.trim().is_empty() {
        parts.push(RawPart::Raw(raw.to_string()));
    } else {
        parts.push(RawPart::Text(decoded));
    }
}

/// Decodes the WebVTT character references used by the document layer.
///
/// Unknown references are preserved literally rather than discarded.
///
/// # Arguments
///
/// * `raw` - Raw text run.
fn decode_entities(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut remaining = raw;
    while let Some(ampersand) = remaining.find('&') {
        output.push_str(&remaining[..ampersand]);
        let after = &remaining[ampersand..];
        if let Some(end) = after.find(';') {
            if let Some(character) = decode_entity(&after[1..end]) {
                output.push(character);
                remaining = &after[end + 1..];
                continue;
            }
        }
        output.push('&');
        remaining = &after[1..];
    }
    output.push_str(remaining);
    output
}

/// Decodes one named, decimal or hexadecimal WebVTT character reference.
///
/// # Arguments
///
/// * `entity` - Reference body without the surrounding `&` and `;`.
fn decode_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "lrm" => Some('\u{200E}'),
        "rlm" => Some('\u{200F}'),
        "nbsp" => Some('\u{00A0}'),
        _ if entity.starts_with("#x") || entity.starts_with("#X") => {
            u32::from_str_radix(&entity[2..], 16)
                .ok()
                .and_then(char::from_u32)
        }
        _ if entity.starts_with('#') => entity[1..].parse::<u32>().ok().and_then(char::from_u32),
        _ => None,
    }
}

/// Splits one text run into blocks that each fit a single translation request.
///
/// Whitespace between split pieces is removed from the blocks and recorded as a
/// joiner so reconstruction can restore the original spacing. Unicode
/// characters are never cut.
///
/// # Arguments
///
/// * `text` - Decoded translatable text.
/// * `prefix` - Stable block identifier prefix for this text run.
/// * `capability` - Selected text translation capability.
/// * `local` - Local translation bounds.
///
/// # Errors
///
/// Propagates translation identifier or size errors.
fn split_text_run(
    text: &str,
    prefix: &str,
    capability: &TextTranslationCapability,
    local: &LocalTranslationLimits,
) -> Result<Vec<TextPiece>, SubtitleTranslationError> {
    let mut pieces: Vec<TextPiece> = Vec::new();
    let mut remaining = text;
    let mut part_index = 0usize;

    while !remaining.is_empty() {
        if remaining.trim().is_empty() {
            if let Some(last) = pieces.last_mut() {
                last.joiner_after.push_str(remaining);
            }
            break;
        }

        let id = block_id(prefix, part_index)?;
        let block = TextBlock::new(id.clone(), remaining)?;
        if plan_batches(std::slice::from_ref(&block), capability, local).is_ok() {
            pieces.push(TextPiece {
                block,
                joiner_after: String::new(),
            });
            break;
        }

        let max_end = max_fitting_prefix(remaining, &id, capability, local)?;
        if max_end == 0 {
            return Err(TranslationError::SizeLimit {
                detail: format!("text run {prefix} cannot fit in one translation request"),
            }
            .into());
        }

        let (mut head_end, mut tail_start) =
            choose_split_boundary(remaining, max_end).unwrap_or((max_end, max_end));
        if head_end == 0 || remaining[..head_end].trim().is_empty() {
            head_end = max_end;
            tail_start = max_end;
        }

        let head = TextBlock::new(id, &remaining[..head_end])?;
        pieces.push(TextPiece {
            block: head,
            joiner_after: remaining[head_end..tail_start].to_string(),
        });
        part_index += 1;
        remaining = &remaining[tail_start..];
    }

    Ok(pieces)
}

/// Builds a stable block identifier for one split part.
///
/// # Arguments
///
/// * `prefix` - Text run prefix.
/// * `part_index` - Zero-based split part index.
///
/// # Errors
///
/// Returns a translation identifier error when the generated value is invalid.
fn block_id(prefix: &str, part_index: usize) -> Result<BlockId, TranslationError> {
    BlockId::new(format!("{prefix}:part-{part_index}"))
}

/// Returns whether one text run fits a single translation request.
///
/// # Arguments
///
/// * `text` - Candidate block text.
/// * `id` - Candidate block identifier.
/// * `capability` - Selected text translation capability.
/// * `local` - Local translation bounds.
///
/// # Errors
///
/// Propagates invalid block identifier or text errors.
fn fits(
    text: &str,
    id: &BlockId,
    capability: &TextTranslationCapability,
    local: &LocalTranslationLimits,
) -> Result<bool, TranslationError> {
    if text.trim().is_empty() {
        return Ok(true);
    }
    let block = TextBlock::new(id.clone(), text)?;
    Ok(plan_batches(std::slice::from_ref(&block), capability, local).is_ok())
}

/// Finds the byte offset of the longest prefix that fits one request.
///
/// # Arguments
///
/// * `text` - Text run that does not fit as a whole.
/// * `id` - Block identifier used by the fit probe.
/// * `capability` - Selected text translation capability.
/// * `local` - Local translation bounds.
///
/// # Errors
///
/// Propagates identifier or block construction errors from the probe.
fn max_fitting_prefix(
    text: &str,
    id: &BlockId,
    capability: &TextTranslationCapability,
    local: &LocalTranslationLimits,
) -> Result<usize, SubtitleTranslationError> {
    let boundaries: Vec<usize> = text
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(text.len()))
        .collect();
    let mut low = 0usize;
    let mut high = boundaries.len() - 1;
    while low < high {
        let middle = (low + high).div_ceil(2);
        if fits(&text[..boundaries[middle]], id, capability, local)? {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    Ok(boundaries[low])
}

/// Chooses a sentence or word boundary near the end of a fitting prefix.
///
/// # Arguments
///
/// * `text` - Remaining text run.
/// * `max_end` - Largest byte offset that still fits.
///
/// Returns the head end and the start of the following text, with the
/// whitespace between them acting as the joiner.
fn choose_split_boundary(text: &str, max_end: usize) -> Option<(usize, usize)> {
    let minimum = max_end / 2;
    let characters: Vec<(usize, char)> = text[..max_end].char_indices().collect();
    let mut last_sentence = None;
    let mut last_whitespace = None;

    for pair in characters.windows(2) {
        let (_, character) = pair[0];
        let (next_index, next_character) = pair[1];
        if character.is_whitespace() || !next_character.is_whitespace() {
            continue;
        }
        if next_index < minimum {
            continue;
        }
        last_whitespace = Some(next_index);
        if is_sentence_end(character) {
            last_sentence = Some(next_index);
        }
    }

    let boundary = last_sentence.or(last_whitespace)?;
    let mut tail_start = boundary;
    while tail_start < text.len() {
        let character = text[tail_start..].chars().next()?;
        if !character.is_whitespace() {
            break;
        }
        tail_start += character.len_utf8();
    }
    Some((boundary, tail_start))
}

/// Returns whether a character commonly ends a sentence.
///
/// # Arguments
///
/// * `character` - Last non-whitespace character before a boundary.
fn is_sentence_end(character: char) -> bool {
    matches!(
        character,
        '.' | '!' | '?' | '…' | '。' | '！' | '？' | ';' | ':' | '；' | '：'
    )
}

/// Rebuilds one cue payload from translated segments and raw parts.
///
/// # Arguments
///
/// * `planned` - Planned cue parts.
/// * `rendered` - Sanitized and escaped translated text per block index.
fn render_payload(planned: &PlannedCue, rendered: &[String]) -> String {
    let mut output = String::new();
    for part in &planned.parts {
        match part {
            PlannedPart::Raw(text) => output.push_str(text),
            PlannedPart::Segments(segments) => {
                for segment in segments {
                    if let Some(text) = rendered.get(segment.block_index) {
                        output.push_str(text);
                    }
                    output.push_str(&segment.joiner_after);
                }
            }
        }
    }
    output
}

/// Normalizes and escapes one translated segment.
///
/// Carriage returns become line feeds, NUL characters are dropped and empty
/// lines are removed so translated text cannot terminate a cue block.
///
/// # Arguments
///
/// * `text` - Raw text returned by the translation service.
fn sanitize_translation(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut output = String::with_capacity(normalized.len());
    let mut first_line = true;
    for line in normalized.split('\n').filter(|line| !line.is_empty()) {
        if !first_line {
            output.push('\n');
        }
        first_line = false;
        for character in line.chars().filter(|character| *character != '\0') {
            match character {
                '&' => output.push_str("&amp;"),
                '<' => output.push_str("&lt;"),
                '>' => output.push_str("&gt;"),
                _ => output.push(character),
            }
        }
    }
    output
}

/// Builds a contextualized invalid translation-response error.
///
/// # Arguments
///
/// * `detail` - Human-readable cause.
fn invalid_response(detail: impl Into<String>) -> SubtitleTranslationError {
    SubtitleTranslationError::Translation(TranslationError::InvalidResponse {
        detail: detail.into(),
    })
}
