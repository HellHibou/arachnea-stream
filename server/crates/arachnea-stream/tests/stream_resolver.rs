use super::test_support::*;
use super::{ResolvedStream, StreamResolver};
use crate::test_support::configure_test_application_data_dir;
use std::sync::atomic::Ordering;

#[test]
fn resolve_stream_ignores_missing_query_and_uses_first_valid_service() {
    let missing_resolve_stream = r#"
id: missing-resolve-stream
queries:
  - name: can_resolve_url
    scraper_type: static
    entries:
      - name: resolver
        type: string
        value: "{service_id}"
  - name: service_stream_metadata
    scraper_type: static
    entries:
      - name: title
        type: string
        value: "ignored"
"#;

    let first_valid = r#"
id: first-valid
queries:
  - name: can_resolve_url
    scraper_type: static
    entries:
      - name: resolver
        type: string
        value: "{service_id}"
  - name: resolve_stream
    scraper_type: static
    entries:
      - name: stream_url
        type: string
        value: "https://cdn.test/first.m3u8"
"#;

    let second_valid = r#"
id: second-valid
queries:
  - name: can_resolve_url
    scraper_type: static
    entries:
      - name: resolver
        type: string
        value: "{service_id}"
  - name: resolve_stream
    scraper_type: static
    entries:
      - name: stream_url
        type: string
        value: "https://cdn.test/second.m3u8"
"#;

    let (agregator, temp_dir) = setup_resolver_sources(&[
        ("missing", missing_resolve_stream),
        ("first", first_valid),
        ("second", second_valid),
    ]);
    let endpoints = crate::services::player_resolver::PlayerResolverEndpoints::default();
    let resolver = StreamResolver::new(&agregator, &endpoints);

    let result = block_on(resolver.get_stream("https://embed.test/video"))
        .expect("resolver should return a stream");

    match result {
        ResolvedStream::Stream(stream) => {
            assert_eq!(stream.stream_url, vec!["https://cdn.test/first.m3u8"]);
        }
        ResolvedStream::EmbedLink { .. } => panic!("expected first valid stream"),
    }

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn html_fallback_fetches_once_detects_service_and_reuses_input_html() {
    let html_body = r#"<html><body>DETECTME stream=https://cdn.test/fallback.m3u8</body></html>"#;
    let (url, request_count) = serve_html_once(html_body);

    let html_resolver = r#"
id: html-resolver
parameters:
  - name: url
    value: ""
  - name: html
    value: ""
queries:
  - name: can_resolve_html
    scraper_type: static
    entries:
      - name: resolver
        type: string
        value: "{html}"
        actions:
          - type: regex_find_all
            pattern: 'DETECTME'
            format: "{service_id}"
  - name: resolve_stream
    scraper_type: html
    base_url: ""
    query_url: ""
    input_html: "{html}"
    row_selector: "html"
    entries:
      - name: stream_url
        type: string
        actions:
          - type: get_response_body
          - type: regex_find_all
            pattern: 'stream=(https://cdn\.test/fallback\.m3u8)'
            format: "{1}"
      - name: stream_headers
        type: object
        select: first
        entries:
          - name: Referer
            type: string
            actions:
              - type: format_text
                argument: "https://embed.test/source"
      - name: manifest_type
        type: string
        actions:
          - type: format_text
            argument: "hls"
"#;

    let (agregator, temp_dir) = setup_resolver_sources(&[("html", html_resolver)]);
    let mut endpoints = crate::services::player_resolver::PlayerResolverEndpoints::default();
    endpoints.http_proxy_public_path = Some("/api/proxy".to_string());
    let resolver = StreamResolver::new(&agregator, &endpoints);

    let can_resolve = block_on(resolver.can_resolve_html("html-resolver", &url, html_body))
        .expect("HTML resolver detection should execute");
    assert!(can_resolve, "HTML resolver should recognize fixture body");

    let result = block_on(resolver.get_stream(&url)).expect("HTML fallback should resolve");

    match result {
        ResolvedStream::Stream(stream) => {
            assert_eq!(stream.manifest_type.as_deref(), Some("hls"));
            assert_eq!(stream.stream_url.len(), 1);
            assert!(stream.stream_url[0].starts_with("/api/proxy/opts_"));
            assert!(stream.stream_url[0].contains("/https://cdn.test/fallback.m3u8"));
            let opts = decode_proxy_opts(&stream.stream_url[0]);
            assert_eq!(
                opts["headers"],
                serde_json::json!([["Referer", format!("{url}/")]])
            );
            let public_json = serde_json::to_string(&stream).expect("stream should serialize");
            assert!(!public_json.contains(html_body));
            assert!(!public_json.contains("DETECTME"));
            assert!(!public_json.contains("stream_headers"));
            assert!(!public_json.contains("Referer"));
        }
        ResolvedStream::EmbedLink { .. } => panic!("expected HTML fallback stream"),
    }
    assert_eq!(request_count.load(Ordering::SeqCst), 1);

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn resolve_stream_keeps_proxied_subtitle_items_and_filters_storyboards() {
    configure_test_application_data_dir();

    let html_body = concat!(
        "<html><body>",
        "stream=https://cdn.test/fallback.m3u8\n",
        "track kind=subtitles lang=eng label=English url=https://cdn.test/subtitles/en.vtt\n",
        "track kind=storyboard lang= label=sprite url=https://cdn.test/storyboards/sprite.vtt\n",
        "track kind=subtitles lang=fre label=French url=https://cdn.test/subtitles/fr.vtt\n",
        "track kind=subtitles lang=eng label=English url=https://cdn.test/subtitles/en.vtt",
        "</body></html>"
    );
    let (url, request_count) = serve_html_once(html_body);

    let resolver_yaml = r#"
id: subtitle-resolver
queries:
  - name: can_resolve_url
    scraper_type: static
    entries:
      - name: resolver
        type: string
        value: "{service_id}"
  - name: resolve_stream
    scraper_type: html
    base_url: "{url}"
    query_url: "{url}"
    row_selector: "html"
    entries:
      - name: stream_url
        type: string
        actions:
          - type: get_response_body
          - type: regex_find_all
            pattern: 'stream=(https://cdn\.test/fallback\.m3u8)'
            format: "{1}"
      - name: _subtitle_tracks_source
        type: string
        actions:
          - type: get_response_body
    post_process:
      - type: extract_regex_items
        source: _subtitle_tracks_source
        target: subtitles
        pattern: 'track kind=([^ ]+) lang=([^ ]*) label=([^ ]+) url=([^ ]+)'
        entries:
          - name: kind
            type: string
            capture_group: 1
          - name: lang
            type: string
            capture_group: 2
          - name: label
            type: string
            capture_group: 3
          - name: link
            type: string
            capture_group: 4
            actions:
              - type: resolve_url
                proxy: true
                proxy_headers:
                  Referer: "{url}"
      - type: filter_items
        source: subtitles
        field: kind
        pattern: '^subtitles$'
      - type: remove_fields
        fields:
          - _subtitle_tracks_source
"#;

    let (agregator, temp_dir) = setup_resolver_sources(&[("subtitles", resolver_yaml)]);
    let mut endpoints = crate::services::player_resolver::PlayerResolverEndpoints::default();
    endpoints.http_proxy_public_path = Some("/api/http-proxy".to_string());
    let resolver = StreamResolver::new(&agregator, &endpoints);

    let result = block_on(resolver.get_stream(&url)).expect("resolver should return a stream");

    match result {
        ResolvedStream::Stream(stream) => {
            assert_eq!(stream.subtitles.len(), 2);
            assert_eq!(stream.subtitles[0].lang.as_deref(), Some("eng"));
            assert_eq!(stream.subtitles[0].label.as_deref(), Some("English"));
            assert_eq!(stream.subtitles[1].lang.as_deref(), Some("fre"));
            assert_eq!(stream.subtitles[1].label.as_deref(), Some("French"));

            for (subtitle, upstream_url) in stream.subtitles.iter().zip([
                "https://cdn.test/subtitles/en.vtt",
                "https://cdn.test/subtitles/fr.vtt",
            ]) {
                assert!(subtitle.link.starts_with("/api/http-proxy/opts_"));
                assert!(
                    subtitle.link.contains(upstream_url),
                    "proxied subtitle URL should contain its upstream URL: {}",
                    subtitle.link
                );
                let opts = decode_proxy_opts(&subtitle.link);
                assert_eq!(opts["headers"], serde_json::json!([["Referer", url]]));
            }

            let public_json = serde_json::to_string(&stream).expect("stream should serialize");
            assert!(public_json.contains("subtitles"));
            assert!(!public_json.contains("storyboards/sprite.vtt"));
            assert!(!public_json.contains("_subtitle_tracks_source"));
        }
        ResolvedStream::EmbedLink { .. } => panic!("expected stream with subtitles"),
    }
    assert_eq!(request_count.load(Ordering::SeqCst), 1);

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn html_fallback_returns_embed_link_when_no_yaml_recognizes_document() {
    let (url, request_count) = serve_html_once("<html><body>unknown hoster</body></html>");

    let non_matching_resolver = r#"
id: non-matching
parameters:
  - name: html
    value: ""
queries:
  - name: can_resolve_html
    scraper_type: static
    entries:
      - name: resolver
        type: string
        value: "{html}"
        actions:
          - type: regex_find_all
            pattern: 'NEVER_MATCH_THIS_DOCUMENT'
            format: "{service_id}"
"#;

    let (agregator, temp_dir) = setup_resolver_sources(&[("non_matching", non_matching_resolver)]);
    let endpoints = crate::services::player_resolver::PlayerResolverEndpoints::default();
    let resolver = StreamResolver::new(&agregator, &endpoints);

    let result = block_on(resolver.get_stream(&url)).expect("unknown HTML should fallback");

    match result {
        ResolvedStream::EmbedLink { embed_link, .. } => assert_eq!(embed_link, url),
        ResolvedStream::Stream(_) => panic!("expected embed-link fallback"),
    }
    assert_eq!(request_count.load(Ordering::SeqCst), 1);

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn html_fallback_follows_simple_js_redirect_before_detection() {
    let shim = r#"<html><head><title>Redirecting...</title></head><body><script>window.location.href = '/player';</script></body></html>"#;
    let final_body = r#"<html><body>DETECTME stream=https://cdn.test/fallback.m3u8</body></html>"#;
    let (url, request_count) = serve_js_redirect_chain(shim, final_body);

    let html_resolver = r#"
id: html-resolver
parameters:
  - name: url
    value: ""
  - name: html
    value: ""
queries:
  - name: can_resolve_html
    scraper_type: static
    entries:
      - name: resolver
        type: string
        value: "{html}"
        actions:
          - type: regex_find_all
            pattern: 'DETECTME'
            format: "{service_id}"
  - name: resolve_stream
    scraper_type: html
    base_url: ""
    query_url: ""
    input_html: "{html}"
    row_selector: "html"
    entries:
      - name: stream_url
        type: string
        actions:
          - type: get_response_body
          - type: regex_find_all
            pattern: 'stream=(https://cdn\.test/fallback\.m3u8)'
            format: "{1}"
      - name: stream_headers
        type: object
        select: first
        entries:
          - name: Referer
            type: string
            actions:
              - type: format_text
                argument: "https://embed.test/source"
      - name: manifest_type
        type: string
        actions:
          - type: format_text
            argument: "hls"
"#;

    let (agregator, temp_dir) = setup_resolver_sources(&[("html", html_resolver)]);
    let endpoints = crate::services::player_resolver::PlayerResolverEndpoints::default();
    let resolver = StreamResolver::new(&agregator, &endpoints);

    let result = block_on(resolver.get_stream(&url)).expect("HTML fallback should resolve");

    match result {
        ResolvedStream::Stream(stream) => {
            assert_eq!(stream.manifest_type.as_deref(), Some("hls"));
            assert_eq!(stream.stream_url, vec!["https://cdn.test/fallback.m3u8"]);
        }
        ResolvedStream::EmbedLink { .. } => panic!("expected stream after JS redirect"),
    }
    assert_eq!(
        request_count.load(Ordering::SeqCst),
        2,
        "expected the shim and the final player page to be fetched"
    );

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn html_fallback_does_not_follow_branchless_non_literal_js_assignment() {
    let body =
        r#"<html><script>window.location.href = getUrl();</script>UNKNOWN content here</html>"#;
    let (url, request_count) = serve_html_once(body);

    let non_matching_resolver = r#"
id: non-matching
parameters:
  - name: html
    value: ""
queries:
  - name: can_resolve_html
    scraper_type: static
    entries:
      - name: resolver
        type: string
        value: "{html}"
        actions:
          - type: regex_find_all
            pattern: 'NEVER_MATCH_THIS_DOCUMENT'
            format: "{service_id}"
"#;

    let (agregator, temp_dir) = setup_resolver_sources(&[("non_matching", non_matching_resolver)]);
    let endpoints = crate::services::player_resolver::PlayerResolverEndpoints::default();
    let resolver = StreamResolver::new(&agregator, &endpoints);

    let result = block_on(resolver.get_stream(&url)).expect("unknown HTML should fallback");

    match result {
        ResolvedStream::EmbedLink { embed_link, .. } => assert_eq!(embed_link, url),
        ResolvedStream::Stream(_) => panic!("expected embed-link fallback"),
    }
    assert_eq!(
        request_count.load(Ordering::SeqCst),
        1,
        "non-literal JS assignment should not be followed"
    );

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn voe_can_resolve_html_detects_proposed_domains_in_memory() {
    let voe_yaml = include_str!("../../../services/arachnea-stream-hoster/voe.yaml");
    let (agregator, temp_dir) = setup_resolver_sources(&[("voe", voe_yaml)]);
    let endpoints = crate::services::player_resolver::PlayerResolverEndpoints::default();
    let resolver = StreamResolver::new(&agregator, &endpoints);

    for domain in [
        "voe.sx",
        "voe.com",
        "voe.ru",
        "voesx.sx",
        "voesx.com",
        "voesx.ru",
        "jessicayeahcatch.com",
        "jeanprofessorcentral.com",
        "juliewomanwish.com",
        "garylargeavailable.com",
        "jennifereconomicgive.com",
        "pamelachangemission.com",
        "ellenpoliticalfollow.com",
        "ralphysuccessfull.org",
    ] {
        let html = format!(
            r#"<html><head><meta name="keywords" content="VOE"></head><body>https://{domain}/e/example</body></html>"#
        );
        let can_resolve =
            block_on(resolver.can_resolve_html("voe", "https://embed.test/player", &html))
                .expect("VOE can_resolve_html should execute");
        assert!(can_resolve, "VOE should detect {domain} in provided HTML");
    }

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn voe_can_resolve_html_rejects_generic_encoded_config_marker() {
    let voe_yaml = include_str!("../../../services/arachnea-stream-hoster/voe.yaml");
    let (agregator, temp_dir) = setup_resolver_sources(&[("voe", voe_yaml)]);
    let endpoints = crate::services::player_resolver::PlayerResolverEndpoints::default();
    let resolver = StreamResolver::new(&agregator, &endpoints);

    let html = r#"<html><script type="application/json">["encodedVoePayload"]</script></html>"#;
    let can_resolve =
        block_on(resolver.can_resolve_html("voe", "https://unrelated.example/e/example", html))
            .expect("VOE can_resolve_html should execute");

    assert!(
        !can_resolve,
        "VOE should not detect a page without the VOE keywords meta tag"
    );

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn get_stream_rejects_non_http_url() {
    let (agregator, temp_dir) = setup_resolver_sources(&[]);
    let endpoints = crate::services::player_resolver::PlayerResolverEndpoints::default();
    let resolver = StreamResolver::new(&agregator, &endpoints);

    let result = block_on(resolver.get_stream("ftp://invalid.protocol"));
    assert!(result.is_err(), "get_stream should reject non-HTTP(S) URLs");

    let _ = std::fs::remove_dir_all(temp_dir);
}
