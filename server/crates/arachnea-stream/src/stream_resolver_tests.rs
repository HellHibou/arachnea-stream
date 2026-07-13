use arachnea_scrapyfy::ScraperAgregator;

use super::StreamResolver;
use crate::stream_scraper::StreamScraper;

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Runtime::new()
        .expect("Failed to create tokio runtime for tests")
        .block_on(future)
}

/// Initializes a scraper and returns it along with a resolver bound to its agregator.
/// The scraper is kept alive so the agregator reference remains valid for the test.
fn setup_scraper_and_agregator() -> (StreamScraper, *const ScraperAgregator) {
    let scraper = StreamScraper::from_json(None).expect("Failed to create test scraper");
    let ptr: *const ScraperAgregator = &*scraper.scraper_agregator;
    (scraper, ptr)
}

#[test]
fn test_sibnet_can_resolve_url() {
    let (_scraper, agregator_ptr) = setup_scraper_and_agregator();
    // SAFETY: the agregator lives as long as _scraper which is dropped at end of scope.
    let agregator = unsafe { &*agregator_ptr };
    let endpoints = crate::services::player_resolver::PlayerResolverEndpoints::default();
    let resolver = StreamResolver::new(agregator, &endpoints);

    let result = block_on(resolver.aggregate_can_resolve(
        "https://video.sibnet.ru/some_video_id",
    ));
    assert!(result.is_ok(), "can_resolve_url should succeed for sibnet");
    let resolved = result.unwrap();
    assert!(
        resolved.can_resolve,
        "can_resolve_url should find sibnet resolver"
    );
    assert_eq!(
        resolved.resolver.as_deref(),
        Some("sibnet"),
        "resolver should be 'sibnet'"
    );
}

#[test]
fn test_sibnet_cannot_resolve_unknown_url() {
    let (_scraper, agregator_ptr) = setup_scraper_and_agregator();
    let agregator = unsafe { &*agregator_ptr };
    let endpoints = crate::services::player_resolver::PlayerResolverEndpoints::default();
    let resolver = StreamResolver::new(agregator, &endpoints);

    let result = block_on(resolver.aggregate_can_resolve(
        "https://example.com/some_video",
    ));
    assert!(result.is_ok(), "can_resolve_url should succeed for unknown");
    let resolved = result.unwrap();
    assert!(
        !resolved.can_resolve,
        "can_resolve_url should NOT find resolver for unknown URL"
    );
}

#[test]
fn test_get_stream_embed_fallback_for_unknown_url() {
    let (_scraper, agregator_ptr) = setup_scraper_and_agregator();
    let agregator = unsafe { &*agregator_ptr };
    let endpoints = crate::services::player_resolver::PlayerResolverEndpoints::default();
    let resolver = StreamResolver::new(agregator, &endpoints);

    let url = "https://unknown-hoster.example/video";
    let result = block_on(resolver.get_stream(url));
    assert!(result.is_ok(), "get_stream should not fail for unknown URL");

    match result.unwrap() {
        crate::stream_resolver::ResolvedStream::EmbedLink { embed_link } => {
            assert_eq!(
                embed_link, url,
                "fallback embed-link should be the original URL"
            );
        }
        _ => panic!("Expected EmbedLink fallback for unknown URL"),
    }
}

#[test]
fn test_get_stream_rejects_non_http_url() {
    let (_scraper, agregator_ptr) = setup_scraper_and_agregator();
    let agregator = unsafe { &*agregator_ptr };
    let endpoints = crate::services::player_resolver::PlayerResolverEndpoints::default();
    let resolver = StreamResolver::new(agregator, &endpoints);

    let result = block_on(resolver.get_stream("ftp://invalid.protocol"));
    assert!(result.is_err(), "get_stream should reject non-HTTP(S) URLs");
}