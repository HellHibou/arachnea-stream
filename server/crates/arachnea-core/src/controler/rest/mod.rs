//! REST controller backend built on top of Warp.

/// Configuration options for the REST controller backend.
pub mod configuration;
/// REST controller service implementation.
mod service;
/// Cooperative shutdown signal shared by the server and the tray.
pub mod shutdown;
/// Generic server tray integration points (contract and GUI detection).
pub mod tray;

pub use configuration::RestControlerConfiguration;
pub use service::RestControlerService;

use warp::filters::BoxedFilter;
use warp::Reply;

type RestReply = (Box<dyn Reply + Send>,);
type RestRouter = BoxedFilter<(RestReply,)>;

/// Builds the public HTTP URL of the running server.
///
/// When the server is bound to an unspecified address (`0.0.0.0` or `::`), the
/// loopback address is used so the generated URL is reachable from the local
/// machine (for example when opened in the default browser).
///
/// # Arguments
/// * `socket_addr` - Socket address the server is bound to.
/// * `entrypoint_root` - Public root path segments.
///
/// # Returns
/// The public URL, using bracket notation for IPv6 addresses.
pub(crate) fn build_public_url(socket_addr: std::net::SocketAddr, entrypoint_root: &[String]) -> String {
    let ip = if socket_addr.ip().is_unspecified() {
        std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
    } else {
        socket_addr.ip()
    };
    let host = if ip.is_ipv6() {
        format!("[{ip}]")
    } else {
        ip.to_string()
    };
    let mut url = format!("http://{host}:{}", socket_addr.port());
    if !entrypoint_root.is_empty() {
        url.push('/');
        url.push_str(&entrypoint_root.join("/"));
    }
    url
}
