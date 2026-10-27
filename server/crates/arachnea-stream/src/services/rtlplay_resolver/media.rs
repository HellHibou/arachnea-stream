//! RTLplay live/replay metadata parsing and stream selection.

use super::*;

pub(super) async fn resolve_final_video_url(
    http_client: &HttpClient,
    session: &RtlPlaySession,
    video_url: &str,
    is_live: bool,
    api_version: &RtlPlayApiVersion,
) -> Result<ResolvedRtlPlayVideo> {
    http_client
        .store_cookies_for_url_in_client(BASE_URL, &rtlplay_player_cookies(&session.cookies))
        .await?;
    let html = http_client
        .query_http_for_request(
            http::Method::GET,
            video_url,
            &header_map_to_hash_map(&rtlplay_headers(api_version)?),
            None,
        )
        .await
        .context("Failed to fetch RTL Play player HTML.")?;

    let next_data =
        extract_next_data_json(&html).context("Missing RTL Play __NEXT_DATA__ payload.")?;
    let page_props = next_data
        .pointer("/props/pageProps")
        .context("Missing RTL Play page props in __NEXT_DATA__.")?;
    let bearer_token = read_json_string(
        page_props,
        &["authToken"][..],
        "Missing RTL Play player bearer token.",
    )?;
    let content_id = if is_live {
        extract_live_content_id(page_props).context("Missing RTL Play live content identifier.")?
    } else {
        read_json_string(
            page_props,
            &["id"][..],
            "Missing RTL Play replay content identifier.",
        )
        .or_else(|_| replay_content_id_from_url(video_url))
        .context("Missing RTL Play replay content identifier.")?
    };

    let config_url = format!(
        "{}?startPosition=0.0&autoPlay=true",
        URL_CONFIG_TEMPLATE.replace("{}", &content_id)
    );

    let mut config_headers = header_map_to_hash_map(&rtlplay_headers(api_version)?);
    config_headers.insert("x-api-key".to_string(), API_KEY.to_string());
    config_headers.insert("popcorn-sdk-version".to_string(), POPCORN_SDK.to_string());
    config_headers.insert(
        "authorization".to_string(),
        format!("Bearer {}", bearer_token),
    );

    let config_body = json!({
        "deviceType": "android-phone",
        "zone": "rtlplay",
    })
    .to_string();

    let payload = http_client
        .get_json_for_request(
            http::Method::POST,
            &config_url,
            &config_headers,
            Some(&config_body),
        )
        .await
        .with_context(|| format!("Failed to fetch RTL Play play-config `{}`.", config_url))?;

    if is_unavailable_payload(&payload) {
        bail!("RTL Play content is not currently available.");
    }

    select_dash_stream(&payload).context("Missing RTL Play DASH stream in play-config response.")
}

pub(super) fn select_dash_stream(payload: &Value) -> Option<ResolvedRtlPlayVideo> {
    let streams = payload.get("video")?.get("streams")?.as_array()?;

    let storyboard_vtt_url = payload
        .get("video")?
        .get("thumbnails")
        .and_then(|t| t.as_array())
        .and_then(|arr| arr.first())
        .and_then(|t| t.get("url"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    for stream in streams {
        let stream_type = stream.get("type").and_then(Value::as_str)?;
        let manifest_url = stream.get("url").and_then(Value::as_str)?;
        if stream_type != "dash" || !manifest_url.contains(".mpd") {
            continue;
        }

        let drm = stream.get("drm").and_then(Value::as_object);
        let widevine = drm.and_then(|drm| {
            drm.iter()
                .find(|(key, _)| key.to_ascii_lowercase().contains("widevine"))
                .map(|(_, value)| value)
        });
        let license_token = widevine
            .and_then(|value| value.pointer("/drmtoday/authToken"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let license_url = widevine
            .and_then(|value| value.get("licenseUrl"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        let mut chapters = Vec::new();

        if let Some(markers) = payload
            .get("video")
            .and_then(|v| v.get("metadata"))
            .and_then(|v| v.get("markers"))
            .and_then(|m| m.as_array())
        {
            for entry in markers.iter() {
                chapters.push(Chapter {
                    chapter_type: entry
                        .get("type")
                        .and_then(|v| v.as_str())
                        .map(|s| match s {
                            "content" => "chapter",
                            "endCredits" => "outro",
                            _ => s,
                        })
                        .unwrap_or("chapter")
                        .to_string(),

                    start: entry
                        .get("start")
                        .and_then(|v| v.as_i64())
                        .map(|v| v as f64)
                        .unwrap_or(0.0),

                    end: entry
                        .get("end")
                        .and_then(|v| v.as_i64())
                        .map(|v| v as f64)
                        .unwrap_or(0.0),

                    title: None,
                });
            }
        }

        return Some(ResolvedRtlPlayVideo {
            manifest_url: manifest_url.to_string(),
            license_url,
            license_token,
            storyboard_vtt_url: storyboard_vtt_url,
            chapters,
        });
    }

    None
}

pub(super) fn extract_next_data_json(html: &str) -> Option<Value> {
    let json = next_data_regex().captures(html)?.get(1)?.as_str().trim();

    serde_json::from_str(json).ok()
}

pub(super) fn next_data_regex() -> &'static Regex {
    NEXT_DATA_REGEX.get_or_init(|| {
        Regex::new(r#"(?s)<script[^>]*\bid=["']__NEXT_DATA__["'][^>]*>(.*?)</script>"#)
            .expect("valid NEXT_DATA regex")
    })
}

pub(super) fn extract_live_content_id(page_props: &Value) -> Option<String> {
    if let (Some(current_path), Some(channels)) = (
        page_props
            .get("currentPath")
            .and_then(json_scalar_to_string),
        page_props.get("channels").and_then(Value::as_array),
    ) {
        for channel in channels {
            let channel_url = channel.get("url").and_then(json_scalar_to_string);
            if channel_url.as_deref() == Some(current_path.as_str()) {
                return channel.get("id").and_then(json_scalar_to_string);
            }
        }
    }

    page_props
        .pointer("/playerData/assetId")
        .and_then(json_scalar_to_string)
        .or_else(|| {
            page_props
                .pointer("/channel/id")
                .and_then(json_scalar_to_string)
        })
}

pub(super) fn replay_content_id_from_url(video_url: &str) -> Result<String> {
    video_url
        .split("/player/")
        .nth(1)
        .and_then(|value| value.split(['?', '/']).next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .context("Missing RTL Play replay content identifier in player URL.")
}

pub(super) fn normalize_live_channel(channel_id: &str) -> Result<String> {
    let trimmed_channel = channel_id.trim();
    let channel_slug =
        live_channel_slug_from_url(trimmed_channel).unwrap_or_else(|| trimmed_channel.to_string());
    let normalized_channel = match channel_slug.as_str() {
        "rtl_tvi" => "tvi",
        "club_rtl" => "club",
        "plug_rtl" => "plug",
        "rtl_info" => "rtl_info",
        "rtl_sport" => "rtl_sport",
        "bel_rtl" => "bel",
        "contact" => "contact",
        "rtl_play" => "rtlplay",
        "rtl_district" => "RTLdistrict",
        "tvi" | "club" | "plug" | "bel" | "rtlplay" | "RTLdistrict" => channel_slug.as_str(),
        _ => "",
    };

    if normalized_channel.is_empty() {
        bail!("Unsupported RTL Play live channel `{}`.", channel_id);
    }

    Ok(normalized_channel.to_string())
}

pub(super) fn live_channel_slug_from_url(channel_id: &str) -> Option<String> {
    let url = Url::parse(channel_id).ok()?;
    if url.domain() != Some("www.rtlplay.be") {
        return None;
    }

    url.path_segments()?
        .filter(|segment| !segment.is_empty())
        .last()
        .map(str::to_string)
}

pub(super) fn is_unavailable_payload(payload: &Value) -> bool {
    let code = payload.get("code").and_then(Value::as_i64);
    let payload_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();

    (code == Some(103) && payload_type.contains("available"))
        || (code == Some(104) && payload_type.contains("found"))
}
