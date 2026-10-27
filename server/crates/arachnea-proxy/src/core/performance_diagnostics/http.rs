//! Optional observation of media request phases and lazy response streams.

use bytes::Bytes;
use std::pin::Pin;

/// Monotonic diagnostic identity independent of reusable search-context addresses.
static NEXT_MEDIA_REQUEST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Owns bounded diagnostics through response-body consumption or early drop.
pub(super) struct MediaRequestDiagnostics {
    id: u64,
    search_id: Option<usize>,
    origin: String,
    started: std::time::Instant,
    phase_started: std::time::Instant,
    phase: &'static str,
    finished: Option<std::time::Instant>,
    bytes: u64,
    outcome: &'static str,
}

impl MediaRequestDiagnostics {
    /// Starts an observation before route selection.
    ///
    /// # Arguments
    /// - `origin`: Scheme and authority only, without URL paths or credentials.
    /// - `method`: Request method included in the initial event.
    /// - `range_requested`: Whether a Range header is present.
    /// - `buffered`: Whether the caller requires complete body buffering.
    pub(super) fn new(
        origin: String,
        method: &str,
        range_requested: bool,
        buffered: bool,
    ) -> Box<Self> {
        let started = std::time::Instant::now();
        let diagnostics = Box::new(Self {
            id: NEXT_MEDIA_REQUEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            search_id: crate::core::ProxySearchContext::current().map(|search| search.id()),
            origin,
            started,
            phase_started: started,
            phase: "selection",
            finished: None,
            bytes: 0,
            outcome: "interrupted",
        });
        tracing::debug!(target: "arachnea_proxy::core::http::client",
            media_request_id = diagnostics.id,
            search_id = diagnostics.search_id,
            origin = %diagnostics.origin,
            method,
            range_requested,
            buffered,
            "started media upstream request"
        );
        diagnostics
    }

    /// Finishes the previous phase and starts the next phase clock.
    ///
    /// # Arguments
    /// - `phase`: Static description of the next runtime phase.
    pub(super) fn next_phase(&mut self, phase: &'static str) {
        tracing::debug!(target: "arachnea_proxy::core::http::client",
            media_request_id = self.id,
            search_id = self.search_id,
            origin = %self.origin,
            phase = self.phase,
            duration_ms = self.phase_started.elapsed().as_millis() as u64,
            "completed media upstream phase"
        );
        self.phase = phase;
        self.phase_started = std::time::Instant::now();
    }
}

impl MediaRequestDiagnostics {
    /// Returns the identity retained by the response-body observer.
    pub(super) fn id(&self) -> u64 {
        self.id
    }

    /// Logs received headers without including request URLs or sensitive headers.
    ///
    /// # Arguments
    /// - `status_code`: Upstream numeric response status.
    pub(super) fn received_headers(&self, status_code: u16) {
        tracing::debug!(target: "arachnea_proxy::core::http::client",
            media_request_id = self.id,
            search_id = self.search_id,
            origin = %self.origin,
            status = status_code,
            "received media upstream headers"
        );
    }

    /// Transfers observation ownership to the lazily polled response body.
    ///
    /// # Arguments
    /// - `stream`: Decoded stream, forwarded without prefetching.
    pub(super) fn observe(
        self: Box<Self>,
        stream: super::response_classifier::ProxyResponseStream,
    ) -> super::response_classifier::ProxyResponseStream {
        Box::pin(ObservedMediaBody {
            inner: stream,
            diagnostics: self,
        })
    }
}

/// Logs classification independently of the observer now owned by the body.
///
/// # Arguments
/// - `media_request_id`: Identity allocated when the request began.
/// - `response_decision`: Classifier result, unchanged by observation.
/// - `classification_started`: Instant captured before inspection.
pub(super) fn classified_response(
    media_request_id: u64,
    response_decision: &super::response_classifier::ProxyResponseDecision,
    classification_started: std::time::Instant,
) {
    use super::response_classifier::ProxyResponseDecision;
    tracing::debug!(target: "arachnea_proxy::core::http::client",
        media_request_id,
        decision = match response_decision {
            ProxyResponseDecision::Accepted => "accepted",
            ProxyResponseDecision::RejectedForDestination { .. } => "rejected",
            ProxyResponseDecision::Inconclusive => "inconclusive",
        },
        duration_ms = classification_started.elapsed().as_millis() as u64,
        "classified media upstream response"
    );
}

impl Drop for MediaRequestDiagnostics {
    fn drop(&mut self) {
        let finished = self.finished.unwrap_or_else(std::time::Instant::now);
        tracing::debug!(target: "arachnea_proxy::core::http::client",
            media_request_id = self.id,
            search_id = self.search_id,
            origin = %self.origin,
            phase = self.phase,
            outcome = self.outcome,
            body_bytes = self.bytes,
            phase_ms = finished.duration_since(self.phase_started).as_millis() as u64,
            total_ms = finished.duration_since(self.started).as_millis() as u64,
            "finished media upstream observation"
        );
    }
}

/// Counts decoded upstream bytes without prefetching or changing stream ownership.
struct ObservedMediaBody {
    inner: super::response_classifier::ProxyResponseStream,
    diagnostics: Box<MediaRequestDiagnostics>,
}

impl futures::Stream for ObservedMediaBody {
    type Item = std::io::Result<Bytes>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let result = self.inner.as_mut().poll_next(cx);
        match &result {
            std::task::Poll::Ready(Some(Ok(bytes))) => {
                if self.diagnostics.bytes == 0 && !bytes.is_empty() {
                    tracing::debug!(target: "arachnea_proxy::core::http::client",
                        media_request_id = self.diagnostics.id,
                        body_wait_ms = self.diagnostics.phase_started.elapsed().as_millis() as u64,
                        "received first media upstream body bytes"
                    );
                }
                self.diagnostics.bytes = self.diagnostics.bytes.saturating_add(bytes.len() as u64);
            }
            std::task::Poll::Ready(Some(Err(_))) => {
                self.diagnostics.outcome = "body_error";
                self.diagnostics
                    .finished
                    .get_or_insert_with(std::time::Instant::now);
            }
            std::task::Poll::Ready(None) if self.diagnostics.outcome != "body_error" => {
                self.diagnostics.outcome = "eof";
                self.diagnostics
                    .finished
                    .get_or_insert_with(std::time::Instant::now);
            }
            _ => {}
        }
        result
    }
}

#[cfg(test)]
#[path = "../../../tests/core/performance_diagnostics/http.rs"]
pub(super) mod tests;
