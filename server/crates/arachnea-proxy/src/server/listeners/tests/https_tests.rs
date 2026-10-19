use super::*;

/// Verifies that multiple certificate PEM blocks are decoded in order.
#[test]
fn parses_pem_certificate_blocks() {
    let pem = "-----BEGIN CERTIFICATE-----\nAQID\n-----END CERTIFICATE-----\n-----BEGIN CERTIFICATE-----\nBAUG\n-----END CERTIFICATE-----";
    let certs = load_pem_certificates(pem).unwrap();
    assert_eq!(certs.len(), 2);
    assert_eq!(certs[0].as_ref(), &[1, 2, 3]);
    assert_eq!(certs[1].as_ref(), &[4, 5, 6]);
}
