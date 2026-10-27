//! TF1 media information fetching and response policy.

use super::*;

pub(super) async fn fetch_media_info(
    http_client: &HttpClient,
    session: &Tf1Session,
    video_id: &str,
    is_live: bool,
) -> Result<Value> {
    let video_url = TF1_MEDIA_INFO_URL_TEMPLATE.replace("{}", video_id.trim());
    let query = if is_live {
        serde_urlencoded::to_string([
            ("context", "MYTF1"),
            ("pver", "5029000"),
            ("platform", "web"),
            ("device", "desktop"),
            ("os", "windows"),
            ("osVersion", "10.0"),
            ("topDomain", "unknown"),
            ("playerVersion", "5.29.0"),
            ("productName", "mytf1"),
            ("productVersion", "3.37.0"),
        ])
    } else {
        serde_urlencoded::to_string([
            ("context", "MYTF1"),
            ("pver", "5010000"),
            ("platform", "web"),
            ("device", "desktop"),
            ("os", "linux"),
            ("osVersion", "unknown"),
            ("topDomain", TF1_BASE_URL),
            ("playerVersion", "5.19.0"),
            ("productName", "mytf1"),
            ("productVersion", "3.22.0"),
        ])
    }
    .context("Failed to serialize the TF1 mediainfo query string.")?;
    let url = format!("{}?{}", video_url, query);

    let headers = HashMap::from([
        (
            "accept".to_string(),
            "application/json, text/plain, */*".to_string(),
        ),
        (
            "authorization".to_string(),
            format!("Bearer {}", session.token),
        ),
        ("origin".to_string(), TF1_BASE_URL.to_string()),
        ("referer".to_string(), format!("{}/", TF1_BASE_URL)),
        (
            "user-agent".to_string(),
            crate::PLAYER_DEFAULT_USER_AGENT.to_string(),
        ),
    ]);

    let response = http_client
        .send_for_request(http::Method::GET, &url, &headers, None)
        .await
        .with_context(|| format!("Failed to fetch TF1 mediainfo for `{}`.", video_id))?;
    let status = response.status();
    tracing::info!(target: "arachnea_stream::services::tf1_resolver",
        video_id,
        is_live,
        http_status = status.as_u16(),
        "received TF1 mediainfo HTTP response"
    );
    let body = response
        .text()
        .await
        .with_context(|| format!("Failed to read TF1 mediainfo response for `{}`.", video_id))?;

    if !status.is_success() {
        bail!(
            "TF1 mediainfo request for `{}` failed with HTTP {}: {}",
            video_id,
            status,
            preview_error_body(&body)
        );
    }

    let media_info: Value = serde_json::from_str(&body).with_context(|| {
        format!(
            "Invalid TF1 mediainfo JSON for `{}` (HTTP {}): {}",
            video_id,
            status,
            preview_error_body(&body)
        )
    })?;
    tracing::info!(target: "arachnea_stream::services::tf1_resolver",
        video_id,
        is_live,
        http_status = status.as_u16(),
        delivery_code = ?media_info.pointer("/delivery/code").and_then(|value| value.as_i64()),
        delivery_country = ?media_info.pointer("/delivery/country").and_then(|value| value.as_str()),
        media_proxy_countries = ?extract_media_proxy_countries(&media_info),
        "parsed TF1 mediainfo geo-routing metadata"
    );
    Ok(media_info)
}

pub(super) fn classify_tf1_status(status: u16) -> ProxyResponseDecision {
    if status == TF1_PROXY_REJECTION_STATUS {
        rejected_for_destination(format!("TF1 HTTP {}", status))
    } else if (200..300).contains(&status) {
        ProxyResponseDecision::Accepted
    } else {
        ProxyResponseDecision::Inconclusive
    }
}

pub(super) fn rejected_for_destination(reason: String) -> ProxyResponseDecision {
    ProxyResponseDecision::RejectedForDestination {
        cooldown: None,
        reason,
    }
}

pub(super) fn is_tf1_mediainfo_url(url: &str) -> bool {
    url.parse::<http::Uri>()
        .ok()
        .and_then(|uri| uri.host().map(str::to_owned))
        .is_some_and(|host| host.eq_ignore_ascii_case("mediainfo.tf1.fr"))
}

pub(super) fn extract_delivery_code(body: &[u8], state: ProxyBodyState) -> Option<i64> {
    if state == ProxyBodyState::Unavailable || body.is_empty() {
        return None;
    }

    let value: Value = serde_json::from_slice(body).ok()?;
    value.pointer("/delivery/code").and_then(Value::as_i64)
}

pub(super) fn extract_media_proxy_countries(media_info: &Value) -> Vec<String> {
    let Some(countries) = media_info
        .pointer("/media/geoList")
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    countries
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .map(str::to_ascii_uppercase)
        .filter(|country| {
            country.len() == 2 && country.bytes().all(|byte| byte.is_ascii_alphabetic())
        })
        .fold(Vec::new(), |mut result, country| {
            if !result.contains(&country) {
                result.push(country);
            }
            result
        })
}

pub(super) fn delivery_rejected(media_info: &Value) -> bool {
    media_info
        .pointer("/delivery/code")
        .and_then(Value::as_i64)
        .is_some_and(|code| code >= 400)
}

pub(super) fn is_proxy_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<ArachneaHttpError>(),
            Some(ArachneaHttpError::Proxy(_))
        )
    })
}

/// Rejects a localized proxy when TF1 reports the geo restriction inside the
/// mediainfo JSON body. TF1 answers with HTTP 200 and a `delivery.code >= 400`
/// for geo-blocked requests, so status-only policies accept the proxy.
pub(super) struct Tf1MediainfoResponseClassifier;

impl ProxyResponseClassifier for Tf1MediainfoResponseClassifier {
    fn body_requirement(&self) -> ProxyBodyRequirement {
        ProxyBodyRequirement::Prefix(MAX_PROXY_RESPONSE_PREFIX)
    }

    fn body_requirement_for_response(
        &self,
        response: &ProxyResponseView<'_>,
    ) -> ProxyBodyRequirement {
        if is_tf1_mediainfo_url(response.requested_url)
            || is_tf1_mediainfo_url(response.effective_url)
        {
            self.body_requirement()
        } else {
            ProxyBodyRequirement::None
        }
    }

    fn classify(
        &self,
        response: &ProxyResponseView<'_>,
    ) -> arachnea_proxy::core::Result<ProxyResponseDecision> {
        let is_mediainfo = is_tf1_mediainfo_url(response.requested_url)
            || is_tf1_mediainfo_url(response.effective_url);

        if !is_mediainfo {
            return Ok(classify_tf1_status(response.status));
        }

        if response.status == TF1_PROXY_REJECTION_STATUS {
            return Ok(rejected_for_destination(format!(
                "TF1 mediainfo HTTP {}",
                response.status
            )));
        }

        if !(200..300).contains(&response.status) {
            return Ok(ProxyResponseDecision::Inconclusive);
        }

        let Some(delivery_code) = extract_delivery_code(response.body, response.body_state) else {
            return Ok(ProxyResponseDecision::Inconclusive);
        };

        if delivery_code >= 400 {
            return Ok(rejected_for_destination(format!(
                "TF1 mediainfo delivery code {}",
                delivery_code
            )));
        }

        Ok(ProxyResponseDecision::Accepted)
    }
}
