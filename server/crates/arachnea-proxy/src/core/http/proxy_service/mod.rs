//! Proxy HTTP handler for the controller stream route.
//!
//! This module implements the endpoint that forwards HTTP requests through
//! the Arachnea proxy core. The path format is:
//!
//! `/<API_PREFIX>/[opts_<BASE64>]/<PROTOCOL>[:<PORT>]/<HOST>/<PATH>[?<QUERY>]`
//!
//! Examples:
//! - `/api/https:8080/www.google.lu/index.html`
//! - `/api/opts_ABCD/https:8080/www.google.lu/search?q=toto`
//! - `/api/https/www.google.lu/images/img.png`

use std::sync::Arc;

use arachnea_core::controler::{ControlerService, ControlerServiceExt};

use crate::core::ArachneaProxyCore;

mod handler;
mod headers;
mod options;
mod redirects;
mod urls;

pub use handler::handle_proxy_http;
pub use urls::{
    proxied_url_with_countries, proxied_url_with_countries_and_insecure_tls,
    proxied_url_with_countries_and_options, proxied_url_with_countries_and_policy,
};

/// Registers the proxy HTTP stream handler on a controller service.
///
/// The handler is registered under `name` and routes through the supplied
/// proxy core. The entry-point URL for `Location` header rewriting is
/// read directly from `ControlerStreamInput::entry_point`, which is populated
/// by the controller backend at request time.
///
/// # Arguments
///
/// - `controler`: Mutable reference to a controller service that will host the handler.
/// - `proxy_core`: Reference to the proxy core used for outbound HTTP requests.
/// - `name`: Name used to register the stream handler.
/// - `fallback_direct`: Default when no country proxy is available; request options may override it.
pub fn register_service(
    controler: &mut dyn ControlerService,
    proxy_core: &ArachneaProxyCore,
    name: &str,
    fallback_direct: bool,
) {
    let core = Arc::new(proxy_core.clone());
    controler.register_stream_function_with_state(name, core, move |core, input| async move {
        handle_proxy_http(core, input, fallback_direct).await
    });
}

#[cfg(test)]
#[path = "../../../../tests/core/http/proxy_service/mod.rs"]
mod tests;
