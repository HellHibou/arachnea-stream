//! Bounded assembly of finite HLS WebVTT subtitle renditions.

use std::collections::HashSet;
use url::Url;

use crate::document::{
    LocalSubtitleLimits, RetrievedSubtitle, SubtitleRetrievalOutcome, SubtitleRetrievalPort,
    SubtitleSourceDescriptor, SubtitleTranslationError, WebVttBlock, WebVttDocument,
};
use crate::AccessScope;

/// Retrieves and merges a finite, unencrypted WebVTT media playlist.
///
/// # Arguments
/// * `retrieval` - Raw segment retriever enforcing destination, proxy and body policies.
/// * `source` - Original rendition descriptor, including its proxy route.
/// * `playlist` - Retrieved media playlist and final upstream URL.
/// * `limits` - Aggregate source document bounds.
/// * `access_scope` - Caller scope forwarded to each segment retrieval.
/// * `route_url` - Application routing applied to resolved upstream segment URLs.
///
/// # Errors
/// Rejects live, encrypted, discontinuous or non-WebVTT playlists, unsupported
/// byte ranges, excessive segments, malformed timing maps and oversized results.
pub async fn assemble(
    retrieval: &dyn SubtitleRetrievalPort,
    source: &SubtitleSourceDescriptor,
    playlist: RetrievedSubtitle,
    limits: LocalSubtitleLimits,
    access_scope: &AccessScope,
    route_url: &(dyn Fn(&str, &Url) -> Result<String, SubtitleTranslationError> + Send + Sync),
) -> Result<SubtitleRetrievalOutcome, SubtitleTranslationError> {
    let text = std::str::from_utf8(&playlist.bytes)
        .map_err(|_| retrieval_error("HLS subtitle playlist is not UTF-8"))?;
    if !text.lines().any(|line| line.trim() == "#EXT-X-ENDLIST") {
        return Err(retrieval_error(
            "subtitle translation requires a finite HLS playlist",
        ));
    }
    let base = Url::parse(playlist.final_url.as_deref().unwrap_or(&source.url))
        .map_err(|_| retrieval_error("invalid HLS subtitle base URL"))?;
    let mut segments = Vec::new();
    let mut duration = None;
    for line in text.lines().map(str::trim) {
        if line.starts_with("#EXT-X-KEY:") && line != "#EXT-X-KEY:METHOD=NONE"
            || line.starts_with("#EXT-X-MAP:")
            || line.starts_with("#EXT-X-BYTERANGE:")
            || line == "#EXT-X-DISCONTINUITY"
            || line.starts_with("#EXT-X-STREAM-INF:")
        {
            return Err(retrieval_error("unsupported HLS subtitle playlist feature"));
        }
        if let Some(value) = line.strip_prefix("#EXTINF:") {
            let seconds = value
                .split(',')
                .next()
                .unwrap_or("")
                .parse::<f64>()
                .map_err(|_| retrieval_error("invalid HLS subtitle duration"))?;
            if !seconds.is_finite() || seconds <= 0.0 {
                return Err(retrieval_error("invalid HLS subtitle duration"));
            }
            duration = Some(seconds * 1000.0);
        } else if !line.is_empty() && !line.starts_with('#') {
            let millis = duration
                .take()
                .ok_or_else(|| retrieval_error("missing HLS segment duration"))?;
            segments.push((
                base.join(line)
                    .map_err(|_| retrieval_error("invalid HLS segment URL"))?,
                millis,
            ));
            if segments.len() > crate::SUBTITLE_HLS_MAX_SEGMENTS {
                return Err(retrieval_error("HLS subtitle segment limit exceeded"));
            }
        }
    }
    if segments.is_empty() || duration.is_some() {
        return Err(retrieval_error("incomplete HLS subtitle playlist"));
    }
    let timestamp_pattern = regex::Regex::new(r"(?:\d{2,}:)?\d{2}:\d{2}\.\d{3}")
        .map_err(|_| retrieval_error("timestamp pattern initialization failed"))?;
    let mut output = WebVttDocument::parse(b"WEBVTT\n\n")?;
    let mut seen = HashSet::new();
    let mut bytes = playlist.bytes.len() as u64;
    let mut elapsed = 0.0;
    let mut origin = None;
    for (url, duration) in segments {
        let mut child = source.clone();
        child.url = route_url(&source.url, &url)?;
        let SubtitleRetrievalOutcome::Modified(segment) =
            retrieval.retrieve(&child, access_scope, None).await?
        else {
            return Err(retrieval_error(
                "unexpected conditional HLS segment response",
            ));
        };
        bytes = bytes.saturating_add(segment.bytes.len() as u64);
        if bytes > limits.max_document_bytes {
            return Err(SubtitleTranslationError::DocumentTooLarge {
                bytes,
                limit: limits.max_document_bytes,
            });
        }
        let document = WebVttDocument::parse(&segment.bytes)?;
        let mut offset = 0i64;
        for metadata in &document.header.metadata {
            if let Some(map) = metadata.strip_prefix("X-TIMESTAMP-MAP=") {
                let mut local = None;
                let mut transport = None;
                for field in map.split(',') {
                    if let Some(value) = field.trim().strip_prefix("LOCAL:") {
                        local = crate::document::vtt::webvtt::parse_timestamp(value);
                    } else if let Some(value) = field.trim().strip_prefix("MPEGTS:") {
                        transport = value
                            .parse::<u64>()
                            .ok()
                            .filter(|value| *value < (1u64 << 33));
                    }
                }
                let local =
                    local.ok_or_else(|| retrieval_error("invalid HLS local timestamp"))? as f64;
                let transport = transport
                    .ok_or_else(|| retrieval_error("invalid HLS transport timestamp"))?
                    as f64
                    / 90.0;
                let reference = *origin.get_or_insert(transport - local - elapsed);
                let wrap = (1u64 << 33) as f64 / 90.0;
                let mapped = transport - local - reference;
                offset = (mapped + ((elapsed - mapped) / wrap).round() * wrap).round() as i64;
            }
        }
        for mut block in document.blocks {
            if let WebVttBlock::Cue(cue) = &mut block {
                // Inline cue timestamps must use the same timeline as the cue.
                cue.timing = shift_timestamps(&cue.timing, offset, &timestamp_pattern)?;
                for payload in &mut cue.payload {
                    *payload = shift_timestamps(payload, offset, &timestamp_pattern)?;
                }
                let identity = (cue.timing.clone(), cue.payload.clone());
                if !seen.insert(identity) {
                    continue;
                }
                if seen.len() as u64 > limits.max_cues {
                    return Err(SubtitleTranslationError::TooManyCues {
                        cues: seen.len() as u64,
                        limit: limits.max_cues,
                    });
                }
            } else if output.blocks.contains(&block) {
                continue;
            }
            output.blocks.push(block);
        }
        elapsed += duration;
    }
    let bytes = output.to_bytes();
    if bytes.len() as u64 > limits.max_document_bytes {
        return Err(SubtitleTranslationError::DocumentTooLarge {
            bytes: bytes.len() as u64,
            limit: limits.max_document_bytes,
        });
    }
    Ok(SubtitleRetrievalOutcome::Modified(RetrievedSubtitle {
        bytes,
        content_type: Some("text/vtt".into()),
        remote_etag: None,
        last_modified: None,
        final_url: None,
    }))
}

/// Shifts cue timing tokens and inline timestamps while preserving all other text.
fn shift_timestamps(
    line: &str,
    offset: i64,
    pattern: &regex::Regex,
) -> Result<String, SubtitleTranslationError> {
    let mut output = String::new();
    let mut end = 0;
    for found in pattern.find_iter(line) {
        // Payload timestamps are enclosed in tags; timing lines contain an arrow.
        if !line.contains("-->")
            && !(line[..found.start()].ends_with('<') && line[found.end()..].starts_with('>'))
        {
            continue;
        }
        output.push_str(&line[end..found.start()]);
        let millis = crate::document::vtt::webvtt::parse_timestamp(found.as_str())
            .ok_or_else(|| retrieval_error("invalid HLS cue timestamp"))?
            as i64
            + offset;
        let millis = millis.max(0) as u64;
        output.push_str(&format!(
            "{:02}:{:02}:{:02}.{:03}",
            millis / 3_600_000,
            millis / 60_000 % 60,
            millis / 1000 % 60,
            millis % 1000
        ));
        end = found.end();
    }
    output.push_str(&line[end..]);
    Ok(output)
}

/// Creates a contextual HLS retrieval error without exposing upstream URLs.
fn retrieval_error(detail: &str) -> SubtitleTranslationError {
    SubtitleTranslationError::RetrievalFailed {
        detail: detail.into(),
    }
}
