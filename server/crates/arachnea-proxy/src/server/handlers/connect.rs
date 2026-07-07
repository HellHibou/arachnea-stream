use crate::core::{ApplicationProtocol, Destination, Result};

/// Parses an HTTP CONNECT authority into a destination.
///
/// Port 443 is treated as HTTPS so dynamic country proxy selection can require
/// upstream HTTPS tunnelling support before choosing a public proxy.
///
/// # Parameters
///
/// - `authority`: CONNECT authority such as `example.com:443`.
///
/// # Returns
///
/// Destination represented by the authority.
///
/// # Errors
///
/// Returns an error when `authority` is not a valid destination authority.
pub fn destination_from_connect_authority(authority: &str) -> Result<Destination> {
    let destination = Destination::from_authority(authority, ApplicationProtocol::Tcp)?;
    if destination.port == 443 {
        Ok(destination.with_protocol(ApplicationProtocol::Https))
    } else {
        Ok(destination)
    }
}
