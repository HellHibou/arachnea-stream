//! RMC/BFM Play manifest and DRM selection and public playback response construction.

use anyhow::{bail, Context, Result};
use arachnea_proxy::http::proxy_service::proxied_url_with_countries;
use arachnea_scrapyfy::HttpClient;
use serde_json::Value;
use std::collections::HashMap;

use super::payload::{
    describe_value, find_all_urls_containing, find_http_string, find_keyed_url,
    find_url_containing, read_opt_string,
};
use super::{playback_headers, SERVICE_ID};
use crate::services::player_resolver::{
    normalize_stream_kind, save_drm_today_license_proxy_url, PlayerResolverEndpoints,
    ResolvedPlayerStream,
};

/// Authenticated playback parts shared by replay and live.
pub(super) struct PlaybackParts {
    manifest_url: String,
    manifest_type: String,
    license: Option<(String, String)>,
    storyboard_vtt_url: Option<String>,
}

/// Builds the public player stream from authenticated playback parts.
///
/// # Arguments
/// * `parts` - Manifest, optional license and storyboard data.
/// * `proxy_countries` - Countries selected for outbound routing.
/// * `endpoints` - Public endpoint settings used to build proxy URLs.
///
/// # Returns
/// The resolved stream with proxied manifest, license and storyboard.
pub(super) fn resolved_stream(
    parts: PlaybackParts,
    proxy_countries: &[String],
    endpoints: &PlayerResolverEndpoints,
) -> Result<ResolvedPlayerStream> {
    let manifest_url = parts.manifest_url;
    let manifest_type = parts.manifest_type;
    // The Widevine license goes through the shared DRM Today proxy (same as
    // M6 Play): `x-dt-auth-token` challenge forwarding with wrapped-license
    // normalization, so the player receives a directly usable license.
    // Clear manifests carry no license pair.
    let license_url = parts.license.map(|(license_url, play_token)| {
        save_drm_today_license_proxy_url(
            endpoints,
            SERVICE_ID,
            &play_token,
            &normalize_stream_kind(None),
            &license_url,
            None,
        )
    });
    let storyboard_vtt_url = parts.storyboard_vtt_url.map(|url| {
        proxied_url_with_countries(
            &url,
            endpoints.http_proxy_public_path.as_deref(),
            proxy_countries,
            &[],
            &[],
        )
    });
    Ok(ResolvedPlayerStream {
        stream_url: vec![proxied_url_with_countries(
            &manifest_url,
            endpoints.http_proxy_public_path.as_deref(),
            proxy_countries,
            &[],
            &[],
        )],
        manifest_type: Some(manifest_type),
        license_url,
        license_headers: HashMap::new(),
        storyboard_vtt_url,
        ..Default::default()
    })
}

/// Extracts playback from an authenticated `video` payload.
///
/// The manifest is always required. The license pair is required only when
/// the manifest itself is encrypted (probed directly); clear manifests
/// return a license-less stream instead of bailing. The storyboard VTT is
/// best-effort and never fails the stream.
///
/// # Arguments
/// * `http_client` - HTTP client used to probe the manifest.
/// * `video` - Authenticated `video` payload.
///
/// # Errors
/// Returns an error when no manifest URL exists or a required license is
/// missing for an encrypted manifest.
///
/// # Returns
/// The extracted playback parts.
pub(super) async fn extract_playback(
    http_client: &HttpClient,
    video: &Value,
) -> Result<PlaybackParts> {
    extract_playback_parts(http_client, video)
        .await
        .with_context(|| {
            format!(
                "RMC BFM Play did not return playback URLs ({})",
                describe_value(video)
            )
        })
}

/// Extracts manifest, license and storyboard candidates from a video payload.
///
/// # Arguments
/// * `http_client` - HTTP client used to probe the manifest.
/// * `video` - Authenticated `video` payload.
///
/// # Errors
/// Returns an error when no manifest exists or an encrypted manifest lacks a
/// license URL or play token.
///
/// # Returns
/// The selected playback parts.
async fn extract_playback_parts(http_client: &HttpClient, video: &Value) -> Result<PlaybackParts> {
    // Collect every manifest candidate: explicit pointers first, then content
    // scans. DASH is preferred over HLS whenever both exist: the shared DRM
    // Today Widevine license path is validated for DASH, while Video.js/VHS
    // cannot drive a Widevine license for SAMPLE-AES HLS (FairPlay on Safari
    // and PlayReady on Edge only), which would load segments without ever
    // decoding them.
    let mut manifest_source = "pointer";
    let pointer_hits: Vec<String> = [
        "/url",
        "/manifest",
        "/manifest_url",
        "/src",
        "/stream_url",
        "/dash_url",
        "/file",
        "/stream/url",
        "/stream/manifest",
        "/stream/dash",
        "/stream/hls",
        "/stream/src",
        "/stream/file",
        "/hls",
    ]
    .iter()
    .filter_map(|pointer| read_opt_string(video, pointer))
    .collect();
    let mpd_urls = find_all_urls_containing(video, ".mpd");
    let m3u8_urls = find_all_urls_containing(video, ".m3u8");
    let manifest_url = pointer_hits
        .iter()
        .find(|url| url.contains(".mpd"))
        .or_else(|| {
            manifest_source = ".mpd scan";
            mpd_urls.first()
        })
        .or_else(|| {
            manifest_source = "pointer-hls";
            pointer_hits.iter().find(|url| url.contains(".m3u8"))
        })
        .or_else(|| {
            manifest_source = ".m3u8 scan";
            m3u8_urls.first()
        })
        .or_else(|| {
            manifest_source = "pointer-other";
            pointer_hits.first()
        })
        .cloned();
    let Some(manifest_url) = manifest_url else {
        bail!(
            "RMC BFM Play video has no manifest URL ({})",
            describe_value(video)
        )
    };
    let manifest_type = manifest_type_from_url(&manifest_url);
    tracing::info!(target: "arachnea_stream::services::rmcbfmplay_resolver",
        manifest_source,
        manifest_type,
        mpd_candidates = mpd_urls.len(),
        m3u8_candidates = m3u8_urls.len(),
        "RMC BFM Play manifest candidates"
    );
    let mut license_source = "pointer";
    let license_url = [
        "/drm/widevine/url",
        "/drm/widevine/license",
        "/drm/widevine/license_url",
        "/drm/license_url",
        "/drm/license",
        "/drm/licence",
        "/license_url",
        "/license",
        "/licence",
    ]
    .iter()
    .find_map(|pointer| read_opt_string(video, pointer))
    .or_else(|| {
        // The Widevine license object may use other field names; prefer any
        // URL from its subtree before falling back to other DRM systems.
        license_source = "widevine subtree";
        find_http_string(video.pointer("/drm/widevine").unwrap_or(&Value::Null))
    })
    .or_else(|| {
        license_source = "widevine-url scan";
        find_url_containing(video, "widevine")
    })
    .or_else(|| {
        license_source = "licen-key scan";
        find_keyed_url(video, "licen")
    });
    let Some(license_url) = license_url else {
        // No license material found: check the manifest itself. Clear
        // manifests play without a license; only encrypted ones bail.
        if !manifest_requires_drm(http_client, &manifest_url, &manifest_type).await {
            tracing::info!(target: "arachnea_stream::services::rmcbfmplay_resolver",
                manifest_source,
                manifest_type,
                video_shape = %describe_value(video),
                "RMC BFM Play clear manifest, no license required"
            );
            return Ok(PlaybackParts {
                manifest_url,
                manifest_type,
                license: None,
                storyboard_vtt_url: extract_storyboard_vtt(video),
            });
        }
        bail!(
            "RMC BFM Play video has no license URL ({})",
            describe_value(video)
        )
    };
    let play_token = ["/drm/play_token", "/play_token", "/drm/token", "/token"]
        .iter()
        .find_map(|pointer| read_opt_string(video, pointer));
    let Some(play_token) = play_token else {
        bail!(
            "RMC BFM Play video has no play token ({})",
            describe_value(video)
        )
    };
    tracing::info!(target: "arachnea_stream::services::rmcbfmplay_resolver",
        manifest_source,
        manifest_type,
        license_source,
        play_token_found = true,
        license_headers = "x-dt-auth-token",
        storyboard_vtt_found = %extract_storyboard_vtt(video).is_some(),
        video_shape = %describe_value(video),
        "RMC BFM Play playback mapping selected"
    );
    Ok(PlaybackParts {
        manifest_url,
        manifest_type,
        license: Some((license_url, play_token)),
        storyboard_vtt_url: extract_storyboard_vtt(video),
    })
}

/// Extracts the storyboard WebVTT URL from the BFF `sprites` field.
///
/// Best-effort only: returns `None` when no VTT turns up, never failing the
/// stream. Only the field layout is logged, never payload values.
///
/// # Arguments
/// * `video` - Authenticated `video` payload.
///
/// # Returns
/// The storyboard VTT URL, or `None` when absent.
fn extract_storyboard_vtt(video: &Value) -> Option<String> {
    let sprites = video.pointer("/sprites").unwrap_or(&Value::Null);
    if let Value::Null = sprites {
        return None;
    }
    if let Some(shape) = sprites.as_object() {
        let keys: Vec<_> = shape.keys().map(String::as_str).collect();
        tracing::info!(target: "arachnea_stream::services::rmcbfmplay_resolver", sprites_keys = %keys.join(","), "RMC BFM Play sprites field");
    }
    ["/url", "/vtt", "/src", "/file"]
        .iter()
        .find_map(|pointer| read_opt_string(sprites, pointer))
        .filter(|url| url.to_ascii_lowercase().contains(".vtt"))
        .or_else(|| find_all_urls_containing(sprites, ".vtt").into_iter().next())
}

/// Probes a manifest for encryption markers (`EXT-X-KEY` for HLS,
/// `ContentProtection` for DASH). Unreadable manifests conservatively count
/// as encrypted so negotiation keeps requiring a license.
///
/// # Arguments
/// * `http_client` - HTTP client used to fetch the manifest.
/// * `manifest_url` - Manifest URL to probe.
/// * `manifest_type` - Manifest type, `m3u8` or `mpd`.
///
/// # Returns
/// `true` when the manifest is encrypted or cannot be read.
async fn manifest_requires_drm(
    http_client: &HttpClient,
    manifest_url: &str,
    manifest_type: &str,
) -> bool {
    let text = match http_client
        .query_http_for_request(http::Method::GET, manifest_url, &playback_headers(), None)
        .await
    {
        Ok(text) => text,
        Err(_) => return true,
    };
    if manifest_type == "m3u8" {
        text.contains("EXT-X-KEY")
    } else {
        text.contains("ContentProtection")
    }
}

/// Determines the manifest type from its URL.
///
/// # Arguments
/// * `url` - Manifest URL.
///
/// # Returns
/// `m3u8` for URLs containing that extension, `mpd` otherwise.
fn manifest_type_from_url(url: &str) -> String {
    if url.to_ascii_lowercase().contains(".m3u8") {
        return "m3u8".to_string();
    }

    "mpd".to_string()
}
