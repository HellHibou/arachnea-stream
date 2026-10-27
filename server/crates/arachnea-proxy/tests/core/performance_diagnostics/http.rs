use super::*;
use futures::StreamExt;

/// Retains the existing client test's observer assertions after extraction.
pub(in crate::core::http) async fn assert_media_body_observation() {
    let mut observed = ObservedMediaBody {
        inner: Box::pin(futures::stream::iter([
            Ok(Bytes::from_static(b"ab")),
            Ok(Bytes::from_static(b"cd")),
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "truncated",
            )),
        ])),
        diagnostics: MediaRequestDiagnostics::new(
            "https://example.test".into(),
            "GET",
            true,
            false,
        ),
    };
    assert_eq!(observed.next().await.unwrap().unwrap(), "ab");
    assert_eq!(observed.next().await.unwrap().unwrap(), "cd");
    assert_eq!(observed.diagnostics.bytes, 4);
    assert_eq!(
        observed.next().await.unwrap().unwrap_err().kind(),
        std::io::ErrorKind::UnexpectedEof
    );
    assert!(observed.next().await.is_none());
    assert_eq!(observed.diagnostics.outcome, "body_error");

    let mut observed = ObservedMediaBody {
        inner: Box::pin(futures::stream::iter([Ok(Bytes::from_static(b"test"))])),
        diagnostics: MediaRequestDiagnostics::new(
            "https://example.test".into(),
            "GET",
            false,
            true,
        ),
    };
    assert_eq!(observed.next().await.unwrap().unwrap(), "test");
    assert!(observed.next().await.is_none());
    assert_eq!(observed.diagnostics.bytes, 4);
    assert_eq!(observed.diagnostics.outcome, "eof");

    let owner = std::sync::Arc::new(());
    let held = owner.clone();
    let mut observed = ObservedMediaBody {
        inner: Box::pin(futures::stream::once(async move {
            let _held = held;
            futures::future::pending::<std::io::Result<Bytes>>().await
        })),
        diagnostics: MediaRequestDiagnostics::new(
            "https://example.test".into(),
            "GET",
            false,
            false,
        ),
    };
    assert!(futures::FutureExt::now_or_never(observed.next()).is_none());
    assert_eq!(observed.diagnostics.outcome, "interrupted");
    drop(observed);
    assert_eq!(std::sync::Arc::strong_count(&owner), 1);
}
