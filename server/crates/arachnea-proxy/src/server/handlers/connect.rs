use crate::core::{ApplicationProtocol, Destination, Result};

/// Parses an HTTP CONNECT authority into a TCP destination.
///
/// # Parameters
///
/// - `authority`: CONNECT authority such as `example.com:443`.
///
/// # Returns
///
/// TCP destination represented by the authority.
///
/// # Errors
///
/// Returns an error when `authority` is not a valid destination authority.
pub fn destination_from_connect_authority(authority: &str) -> Result<Destination> {
    Destination::from_authority(authority, ApplicationProtocol::Tcp)
}
