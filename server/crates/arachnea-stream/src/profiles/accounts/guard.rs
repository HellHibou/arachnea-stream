//! Application authorization applied before any content callback or cache check.

use super::{failure, reject, AccountService};
use anyhow::Result;
use arachnea_core::controler::*;
use std::{collections::HashMap, sync::Arc};

/// Validates that a transport-captured native caller belongs to the public app.
///
/// # Arguments
/// * `context` - Transport-captured request metadata.
pub(super) fn native_public(context: &RequestControlerContext) -> bool {
    context.native_webview().is_some_and(|caller| {
        caller.label != "admin"
            && url::Url::parse(&caller.url).is_ok_and(|url| !url.path().starts_with("/admin"))
    })
}

/// Refuses wrong methods and cross-origin/unverifiable browser mutations.
///
/// # Arguments
/// * `context` - Transport-captured request metadata.
/// * `write` - Whether this operation mutates account state.
pub(crate) fn method(context: &RequestControlerContext, write: bool) -> Result<()> {
    if let Some(actual) = context.method() {
        if actual != if write { "POST" } else { "GET" } {
            return Err(reject(
                "method_not_allowed",
                405,
                "This request method is not allowed.",
            ));
        }
    } else if context.native_webview().is_none() {
        return Err(reject(
            "origin_rejected",
            403,
            "Trusted transport context is required.",
        ));
    }
    if !write || context.native_webview().is_some() {
        return Ok(());
    }
    if browser_origin(context).is_none()
        || context
            .get_header("sec-fetch-site")
            .is_some_and(|site| site != "same-origin" && site != "none")
    {
        return Err(reject(
            "origin_rejected",
            403,
            "Same-origin writes are required.",
        ));
    }

    Ok(())
}

/// Resolves only a same-origin browser scheme, without trusting forwarded headers.
///
/// # Arguments
/// * `context` - Incoming transport metadata including Host and Origin/Referer.
pub(super) fn browser_origin(context: &RequestControlerContext) -> Option<url::Url> {
    let origin = context
        .get_header("origin")
        .or_else(|| context.get_header("referer"))
        .and_then(|value| url::Url::parse(value).ok())?;
    if !matches!(origin.scheme(), "http" | "https") {
        return None;
    }
    let host = context
        .get_header("host")
        .and_then(|host| url::Url::parse(&format!("{}://{host}", origin.scheme())).ok())?;
    (host.host_str() == origin.host_str()
        && host.port_or_known_default() == origin.port_or_known_default())
    .then_some(origin)
}

/// Extracts a browser cookie without accepting public identity from payload fields.
fn cookie_token(context: &RequestControlerContext) -> Option<String> {
    context.get_header("cookie")?.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        (name == crate::PUBLIC_SESSION_COOKIE && value.len() <= 128).then(|| value.to_string())
    })
}

impl AccountService {
    /// Resolves a browser or transport-verified native secret, never returned to JS.
    ///
    /// # Arguments
    /// * `context` - Transport-captured request metadata.
    pub(super) async fn secret(
        &self,
        context: &RequestControlerContext,
    ) -> Result<(Option<String>, bool)> {
        if context.native_webview().is_some() {
            if !native_public(context) {
                return Err(reject(
                    "session_required",
                    401,
                    "Public account authentication is required.",
                ));
            }
            let broker = self.native.clone();
            return tokio::task::spawn_blocking(move || broker.current()).await?;
        }
        if context.remote_addr().is_none() {
            return Err(reject(
                "session_required",
                401,
                "Public account authentication is required.",
            ));
        }
        Ok((cookie_token(context), true))
    }

    /// Authenticates before invoking a content callback, including native/media calls.
    ///
    /// # Arguments
    /// * `context` - Transport-captured request metadata.
    pub(crate) async fn authorize(&self, context: &RequestControlerContext) -> Result<String> {
        let (secret, _) = self.secret(context).await?;
        let secret = secret.ok_or_else(|| {
            reject(
                "session_required",
                401,
                "Public account authentication is required.",
            )
        })?;
        self.authenticate(secret.clone(), false).await?;
        Ok(secret)
    }
}

/// Builds/renews a cookie restricted to this API mount, honoring HTTPS browser origin.
///
/// # Arguments
/// * `token` - Opaque session secret used only in memory.
/// * `context` - Transport-captured request metadata.
/// * `command` - Registered command used to scope the cookie.
pub(super) fn session_cookie(
    token: &str,
    context: &RequestControlerContext,
    command: &str,
) -> String {
    let path = context
        .path()
        .and_then(|p| p.strip_suffix(command))
        .filter(|p| p.starts_with('/') && !p.contains([';', '\r', '\n']))
        .unwrap_or("/");
    let secure = browser_origin(context).is_some_and(|url| url.scheme() == "https");
    format!(
        "{}={token}; Path={path}; HttpOnly; SameSite=Lax; Max-Age={}{}",
        crate::PUBLIC_SESSION_COOKIE,
        if token.is_empty() {
            0
        } else {
            crate::PUBLIC_SESSION_IDLE_TTL_MS / 1000
        },
        if secure { "; Secure" } else { "" }
    )
}

/// Keeps content out of shared caches and lets validated responses revalidate in the browser.
///
/// Responses carrying an ETag are stored in the browser's private cache and
/// revalidated on every use, so the native HTTP cache replays `If-None-Match`
/// after a refresh. Responses without a validator stay uncached.
fn protected_headers(headers: &mut HashMap<String, String>) {
    headers.retain(|name, _| !name.eq_ignore_ascii_case("cache-control"));
    let cacheable = headers
        .keys()
        .any(|name| name.eq_ignore_ascii_case(HEADER_ETAG));
    headers.insert(
        "Cache-Control".into(),
        if cacheable {
            "private, no-cache".into()
        } else {
            "private, no-store".into()
        },
    );
    headers.insert("Vary".into(), "Cookie".into());
}

/// Controller decorator protecting every registered content command by default.
pub struct ContentController<'a> {
    /// Underlying transport controller; session/admin routes register separately.
    pub inner: &'a mut dyn ControlerService,
    /// Persistent public-account authorization service.
    pub accounts: Arc<AccountService>,
}

impl ControlerService for ContentController<'_> {
    fn register_serialized_function(&mut self, name: &str, call: SerializedControlerFunction) {
        let callback: JsonControlerFunction = Arc::new(move |input| {
            let call = call.clone();
            Box::pin(async move { call(input.payload).await.map(ControlerJsonOutput::from) })
        });
        self.register_json_function(name, callback);
    }
    fn register_json_function(&mut self, name: &str, call: JsonControlerFunction) {
        let accounts = self.accounts.clone();
        self.inner.register_json_function(
            name,
            Arc::new(move |input| {
                let accounts = accounts.clone();
                let call = call.clone();
                Box::pin(async move {
                    match accounts.authorize(&input.context).await {
                        Ok(_token) => {
                            let mut out = call(input).await?;
                            protected_headers(&mut out.headers);
                            Ok(out)
                        }
                        Err(error) => {
                            let (status, value) = failure(&error);
                            Ok(ControlerJsonOutput {
                                value,
                                status,
                                headers: HashMap::from([(
                                    "Cache-Control".into(),
                                    "no-store".into(),
                                )]),
                            })
                        }
                    }
                })
            }),
        );
    }
    fn register_stream_function(&mut self, name: &str, call: StreamControlerFunction) {
        let accounts = self.accounts.clone();
        self.inner.register_stream_function(
            name,
            Arc::new(move |input| {
                let accounts = accounts.clone();
                let call = call.clone();
                Box::pin(async move {
                    let mut context = RequestControlerContext::new(input.headers.clone())
                        .with_method(input.method.clone());
                    if let Some(addr) = input.remote_addr {
                        context = context.with_remote_addr(addr);
                    }
                    if let Some(caller) = input.native_webview.clone() {
                        context = context.with_native_webview(caller);
                    }
                    let path = url::Url::parse(&input.entry_point)
                        .map(|u| u.path().to_string())
                        .unwrap_or_else(|_| input.entry_point.clone());
                    context = context.with_path(path);
                    match accounts.authorize(&context).await {
                        Ok(_token) => {
                            let mut out = call(input).await?;
                            protected_headers(&mut out.headers);
                            Ok(out)
                        }
                        Err(error) => {
                            let (status, value) = failure(&error);
                            Ok(ControlerStreamOutput {
                                status,
                                body: ResponseBody::Buffered(
                                    serde_json::to_vec(&value).unwrap_or_default(),
                                ),
                                content_type: "application/json".into(),
                                headers: HashMap::from([(
                                    "Cache-Control".into(),
                                    "no-store".into(),
                                )]),
                            })
                        }
                    }
                })
            }),
        );
    }
    fn stream_public_path(&self, name: &str) -> String {
        self.inner.stream_public_path(name)
    }
    fn register_web_directory(&mut self, directory: &str, path: &str) {
        self.inner.register_web_directory(directory, path)
    }
    fn register_embedded_web_assets(&mut self, assets: SharedWebAssets, path: &str) {
        self.inner.register_embedded_web_assets(assets, path)
    }
    fn main_thread_dispatcher(&self) -> Option<Arc<dyn MainThreadDispatcher>> {
        self.inner.main_thread_dispatcher()
    }
    fn rest_server_handle(&self) -> Option<Arc<RestServerHandle>> {
        self.inner.rest_server_handle()
    }
    fn launch(&mut self) {
        self.inner.launch()
    }
}
