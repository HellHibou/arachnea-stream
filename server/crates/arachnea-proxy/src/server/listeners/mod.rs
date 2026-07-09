//! TCP listeners for inbound proxy protocols.

/// HTTP forward proxy listener.
pub mod http;
/// HTTPS (CONNECT) listener.
pub mod https;
/// SOCKS4/4a/5 listener.
pub mod socks;
