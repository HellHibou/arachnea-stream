use super::*;

/// Verifies that destinations preserve hostnames instead of resolving early.
#[test]
fn destination_preserves_hostname() {
    let destination =
        Destination::from_authority("example.com:443", crate::core::ApplicationProtocol::Https)
            .unwrap();
    assert_eq!(destination.authority(), "example.com:443");
}
