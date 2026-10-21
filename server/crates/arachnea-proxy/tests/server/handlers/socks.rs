use super::*;
use crate::core::{PROXY_HEADER_PARAMETER_COUNTRIES, PROXY_PARAMETER_COUNTRIES};

/// Verifies parsing of a SOCKS5 UDP datagram with an IPv4 destination.
#[test]
fn parses_socks5_udp_ipv4_datagram() {
    let packet = [
        0x00, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0x1f, 0x90, b'p', b'i', b'n', b'g',
    ];
    let datagram = parse_udp_request(&packet).unwrap();
    assert_eq!(datagram.frag, 0);
    assert_eq!(datagram.destination.port, 8080);
    assert_eq!(datagram.payload, b"ping");
}

/// Verifies encoding of a SOCKS5 UDP response with an IPv4 source.
#[test]
fn encodes_socks5_udp_ipv4_response() {
    let source = SocketAddr::from((Ipv4Addr::new(8, 8, 8, 8), 53));
    let packet = encode_udp_response(source, b"pong");
    assert_eq!(
        packet,
        vec![0x00, 0x00, 0x00, 0x01, 8, 8, 8, 8, 0x00, 0x35, b'p', b'o', b'n', b'g',]
    );
}

/// Verifies that SOCKS5 username strings can carry generic parameters.
#[test]
fn extracts_parameters_from_socks_username() {
    let definitions = vec![ParameterDefinition::new(
        PROXY_HEADER_PARAMETER_COUNTRIES,
        PROXY_PARAMETER_COUNTRIES,
        false,
    )];

    let context = context_from_socks_username(r#"countries=["be","BE"]"#, &definitions);

    assert_eq!(
        context.get_string(PROXY_PARAMETER_COUNTRIES),
        Some(r#"["BE"]"#)
    );
}
