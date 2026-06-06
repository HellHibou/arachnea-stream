use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use crate::server::{FileConfig, ServerConfig};

/// Verifies that the default server configuration is loopback-safe.
#[test]
fn default_server_config_is_loopback_safe() {
    assert!(ServerConfig::default().validate().is_ok());
}

/// Verifies that public listeners require an ACL unless explicitly allowed.
#[test]
fn public_bind_without_acl_is_rejected() {
    let mut config = ServerConfig::default();
    config.listen_http = Some(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 8080)));
    config.network.allow_clients.clear();
    assert!(config.validate().is_err());
}

/// Verifies that HTTPS listeners accept complete PEM certificate pairs.
#[test]
fn https_listener_accepts_pem_certificate_pair() {
    let mut config = ServerConfig::default();
    config.listen_https = Some(SocketAddr::from((Ipv4Addr::LOCALHOST, 8443)));
    config.tls_cert_pem_path = Some(PathBuf::from("server-cert.pem"));
    config.tls_key_pem_path = Some(PathBuf::from("server-key.pem"));
    assert!(config.validate().is_ok());
}

/// Verifies that HTTPS listeners reject incomplete TLS path pairs.
#[test]
fn https_listener_rejects_partial_tls_pairs() {
    let mut config = ServerConfig::default();
    config.listen_https = Some(SocketAddr::from((Ipv4Addr::LOCALHOST, 8443)));
    config.tls_cert_pem_path = Some(PathBuf::from("server-cert.pem"));
    assert!(config.validate().is_err());
}

/// Verifies that the country parameter sample is valid TOML and core config.
#[test]
fn country_parameter_sample_is_valid() {
    let config: FileConfig = toml::from_str(include_str!(
        "../../../config-sample/parameters-country-routing.toml"
    ))
    .unwrap();

    assert!(config.server.validate().is_ok());
    assert!(config.core.resolve().is_ok());
}

/// Verifies that the Smart DNS parameter sample is valid TOML and core config.
#[test]
fn smart_dns_parameter_sample_is_valid() {
    let config: FileConfig = toml::from_str(include_str!(
        "../../../config-sample/parameters-smart-dns.toml"
    ))
    .unwrap();

    assert!(config.server.validate().is_ok());
    assert!(config.core.resolve().is_ok());
}
