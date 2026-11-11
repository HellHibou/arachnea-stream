//! Shared decoding of controller proxy routes for in-process HTTP consumers.

use arachnea_core::controler::ControlerStreamInput;
use url::Url;

use super::headers::{
    context_from_header_map, remove_header_variants, strip_non_forwarded_parameter_headers,
};
use super::options::{
    decode_base64url_segment, merge_opts, parse_opts_value, MergedProxyHttpInput, ProxyHttpOpts,
};
use super::redirects::redirect_opts_encoded;
use super::urls::{build_target_url, parse_proxy_path};
use crate::core::http::actions::ProxyHttpRedirectActionConfig;
use crate::core::http::{actions::PostActionContext, ProxiedHttpRequest};
use crate::core::{ArachneaProxyCore, ClientParameter};

/// A decoded application proxy route, including its private redirect options.
///
/// This object contains source credentials and must never be exposed to clients
/// or included in logs. Preparing it performs no network access.
pub struct PreparedProxyHttpSource {
    /// Fully decoded request, using the application's normal proxy semantics.
    pub request: ProxiedHttpRequest,
    /// Original controller input used to prepare redirected requests.
    pub(super) input: ControlerStreamInput,
    /// Encoded options retained for redirect actions.
    pub(super) opts_encoded: String,
    /// Validated options, including explicit retry and TLS policy.
    pub(super) opts: Option<ProxyHttpOpts>,
    /// Status-dependent rules for forwarding encoded options.
    pub(super) redirect_actions: Vec<ProxyHttpRedirectActionConfig>,
}

impl PreparedProxyHttpSource {
    /// Returns the maximum automatic redirect count declared by the URL options.
    pub fn follow_redirects(&self) -> Option<usize> {
        self.opts.as_ref().and_then(|opts| opts.follow_redirects)
    }

    /// Returns the explicit retry replay guarantee declared by the URL options.
    pub fn replayable(&self) -> bool {
        self.opts
            .as_ref()
            .is_some_and(|opts| opts.proxy_retry_replayable)
    }

    /// Decodes the next hop using the existing redirect option actions.
    ///
    /// # Arguments
    /// * `core` - Application proxy core supplying parameter and TLS policy.
    /// * `status` - Redirect response status.
    /// * `location` - Absolute or relative upstream Location value.
    ///
    /// # Errors
    /// Returns a status and contextual decoding error for invalid redirect input.
    pub fn redirect(
        &self,
        core: &ArachneaProxyCore,
        status: u16,
        location: &str,
    ) -> Result<Self, (u16, String)> {
        let target = Url::parse(&self.request.url)
            .and_then(|base| base.join(location))
            .map_err(|_| (400, "invalid proxy redirect destination".into()))?;
        let options = redirect_opts_encoded(
            status,
            &self.opts_encoded,
            self.opts.as_ref(),
            &self.redirect_actions,
        );
        let mut input = copy_input(&self.input);
        input.path = if options.is_empty() {
            target.to_string()
        } else {
            format!("opts_{options}/{target}")
        };
        input.query.clear();
        prepare_proxy_http_source(core, &input)
    }
}

/// Decodes a known controller proxy route for an in-process HTTP consumer.
///
/// It reuses the controller's option, action, cookie and routing parsers. The
/// caller must authorize the source, validate destinations and apply body limits
/// before executing the request. TLS bypass still requires the core allowlist.
///
/// # Arguments
/// * `core` - Application proxy core supplying parameter and TLS policy.
/// * `input` - Controller-shaped request with a route suffix and exact entry point.
///
/// # Errors
/// Returns the HTTP status and contextual error used by the controller decoder.
pub fn prepare_proxy_http_source(
    core: &ArachneaProxyCore,
    input: &ControlerStreamInput,
) -> Result<PreparedProxyHttpSource, (u16, String)> {
    if input
        .entry_point
        .len()
        .saturating_add(input.path.len())
        .saturating_add(input.query.len())
        .saturating_add(1 + usize::from(!input.query.is_empty()))
        > crate::MAX_LOCAL_URL_BYTES
    {
        return Err((414, "proxy source URL is too long".into()));
    }
    let parsed = parse_proxy_path(&input.path)?;
    let target_url = build_target_url(
        &parsed.protocol,
        parsed.port,
        &parsed.host,
        &parsed.path_and_query,
        &input.query,
    );
    let target = Url::parse(&target_url).map_err(|_| (400, "invalid proxy source URL".into()))?;
    if !matches!(target.scheme(), "http" | "https")
        || !target.username().is_empty()
        || target.password().is_some()
    {
        return Err((400, "invalid proxy source URL".into()));
    }
    let method = input.method.to_ascii_uppercase();
    if !crate::PROXY_HTTP_SUPPORTED_METHODS.contains(&method.as_str()) {
        return Err((400, "unsupported proxy request method".into()));
    }
    let opts = if parsed.opts_encoded.is_empty() {
        None
    } else {
        let bytes = decode_base64url_segment(&parsed.opts_encoded).map_err(|error| (400, error))?;
        let value = serde_json::from_slice(&bytes)
            .map_err(|_| (400, "invalid proxy options JSON".into()))?;
        Some(parse_opts_value(&value)?)
    };
    let verify_tls = if opts.as_ref().is_some_and(|opts| opts.insecure_tls) {
        if target.scheme() != "https"
            || !target
                .host_str()
                .is_some_and(|host| core.allows_insecure_tls_for_host(host))
        {
            return Err((403, "TLS bypass is not allowed for this source".into()));
        }
        false
    } else {
        true
    };
    let mut incoming = input.headers.clone();
    if incoming.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("referer") && value.starts_with(&input.entry_point)
    }) {
        remove_header_variants(&mut incoming, "referer");
    }
    let MergedProxyHttpInput {
        mut headers,
        cookies,
        post_actions,
        redirect_actions,
    } = merge_opts(&incoming, opts.clone())?;
    let definitions = core.parameter_definitions();
    let mut client_context = context_from_header_map(&headers, &definitions);
    if let Some(value) = opts.as_ref().and_then(|opts| opts.fallback_direct) {
        client_context.insert(
            crate::core::PROXY_PARAMETER_FALLBACK_DIRECT,
            ClientParameter::Bool(value),
        );
    }
    strip_non_forwarded_parameter_headers(&mut headers, &definitions);
    headers.insert(
        "Host".into(),
        match target.port() {
            Some(port) => format!("{}:{port}", target.host_str().unwrap_or_default()),
            None => target.host_str().unwrap_or_default().to_string(),
        },
    );
    let buffer_response_body =
        crate::core::http::actions::post_actions_require_body_buffering(&post_actions)
            || input.force_buffer_response;
    if buffer_response_body {
        remove_header_variants(&mut headers, "accept-encoding");
        headers.insert("Accept-Encoding".into(), "identity".into());
    }
    let response_classifier = crate::core::http::response_classifier::resolve_response_policy(
        opts.as_ref()
            .and_then(|opts| opts.proxy_response_classifier.as_ref()),
        opts.as_ref()
            .map(|opts| opts.proxy_rejection_statuses.as_slice())
            .unwrap_or_default(),
    )
    .map_err(|error| (400, error.to_string()))?;
    let request = ProxiedHttpRequest {
        response_classifier: Some(response_classifier),
        url: target_url.clone(),
        method: method.clone(),
        headers,
        cookies,
        body: input.body.clone(),
        client_context,
        post_actions,
        buffer_response_body,
        headers_only: method == "HEAD",
        context: PostActionContext {
            entry_point: input.entry_point.clone(),
            target_url,
            opts_encoded: parsed.opts_encoded.clone(),
        },
        verify_tls,
        follow_redirects: opts.as_ref().and_then(|opts| opts.follow_redirects),
    };
    Ok(PreparedProxyHttpSource {
        request,
        input: copy_input(input),
        opts_encoded: parsed.opts_encoded,
        opts,
        redirect_actions,
    })
}

/// Copies the controller input without requiring controller ownership changes.
///
/// # Arguments
/// * `input` - Source request whose owned fields are duplicated.
fn copy_input(input: &ControlerStreamInput) -> ControlerStreamInput {
    ControlerStreamInput {
        path: input.path.clone(),
        query: input.query.clone(),
        method: input.method.clone(),
        headers: input.headers.clone(),
        body: input.body.clone(),
        entry_point: input.entry_point.clone(),
        force_buffer_response: input.force_buffer_response,
    }
}
