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

/// Collects the local network ranges from the machine's interface addresses.
///
/// Each interface address is turned into an `IpNet` from its IP and prefix
/// length. Loopback and link-local addresses are included so loopback clients
/// and local-link IPv6 peers stay allowed.
///
/// # Returns
/// The list of local network ranges, empty when interface enumeration fails.
pub(crate) fn local_networks() -> Vec<ipnet::IpNet> {
    let Ok(interfaces) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };
    interfaces
        .iter()
        .filter_map(|iface| {
            let ip = iface.addr.ip();
            let prefixlen = match iface.addr {
                if_addrs::IfAddr::V4(ref v4) => v4.prefixlen,
                if_addrs::IfAddr::V6(ref v6) => v6.prefixlen,
            };
            ipnet::IpNet::new(ip, prefixlen).ok()
        })
        .collect()
}

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
pub(crate) fn build_public_url(
    socket_addr: std::net::SocketAddr,
    entrypoint_root: &[String],
) -> String {
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

/// Returns the host displayed for a running server.
///
/// Loopback listeners are displayed as `localhost`. Listeners bound to all
/// interfaces use the first non-loopback IPv4 interface when available.
pub(crate) fn display_server_host(socket_addr: std::net::SocketAddr) -> String {
    if socket_addr.ip().is_loopback() {
        return "localhost".to_string();
    }

    if socket_addr.ip().is_unspecified() {
        return if_addrs::get_if_addrs()
            .ok()
            .and_then(|interfaces| {
                interfaces
                    .iter()
                    .find(|iface| {
                        !iface.is_loopback() && matches!(iface.addr, if_addrs::IfAddr::V4(_))
                    })
                    .map(|iface| iface.ip().to_string())
            })
            .unwrap_or_else(|| "localhost".to_string());
    }

    socket_addr.ip().to_string()
}
