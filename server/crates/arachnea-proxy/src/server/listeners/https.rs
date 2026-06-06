use std::net::SocketAddr;
use std::sync::Arc;

use crate::core::ArachneaProxyCore;
use base64::Engine;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig as RustlsServerConfig;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio_rustls::TlsAcceptor;
use tracing::{debug, warn};

use crate::server::handlers;
use crate::server::ServerConfig;

/// Runs the HTTPS proxy listener.
pub async fn run(
    addr: SocketAddr,
    config: Arc<ServerConfig>,
    core: ArachneaProxyCore,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    let acceptor = tls_acceptor(&config)?;
    let acl = config.acl();
    let semaphore = Arc::new(Semaphore::new(
        config.network.max_connections.unwrap_or(1024) as usize,
    ));
    tracing::info!(%addr, "https proxy listener started");

    loop {
        let (stream, peer) = listener.accept().await?;
        if !acl.allows(peer.ip()) {
            warn!(%peer, "https proxy client denied by acl");
            continue;
        }
        let permit = Arc::clone(&semaphore).acquire_owned().await?;
        let conn_config = Arc::clone(&config);
        let conn_core = core.clone();
        let conn_acceptor = acceptor.clone();
        tokio::spawn(async move {
            let _permit = permit;
            match conn_acceptor.accept(stream).await {
                Ok(tls_stream) => {
                    if let Err(error) =
                        handlers::http::handle(tls_stream, peer, conn_config, conn_core).await
                    {
                        debug!(%peer, %error, "https proxy connection ended with error");
                    }
                }
                Err(error) => {
                    debug!(%peer, %error, "https proxy tls handshake failed");
                }
            }
        });
    }
}

/// Builds the TLS acceptor used by the HTTPS proxy listener.
///
/// # Parameters
///
/// - `config`: Server configuration that contains either PEM or DER TLS paths.
///
/// # Returns
///
/// A `TlsAcceptor` configured with the supplied certificate and private key.
///
/// # Errors
///
/// Returns an error when TLS paths are missing, unreadable, or invalid for
/// rustls server authentication.
fn tls_acceptor(config: &ServerConfig) -> anyhow::Result<TlsAcceptor> {
    let (certs, key) = if let (Some(cert_path), Some(key_path)) =
        (&config.tls_cert_pem_path, &config.tls_key_pem_path)
    {
        (
            load_pem_certificates(&std::fs::read_to_string(cert_path)?)?,
            load_pem_private_key(&std::fs::read_to_string(key_path)?)?,
        )
    } else {
        let cert_path = config
            .tls_cert_der_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing tls_cert_der_path"))?;
        let key_path = config
            .tls_key_der_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing tls_key_der_path"))?;
        let cert = std::fs::read(cert_path)?;
        let key = std::fs::read(key_path)?;
        let key = PrivateKeyDer::try_from(key)
            .map_err(|error| anyhow::anyhow!("invalid private key DER: {error}"))?;
        (vec![CertificateDer::from(cert)], key)
    };
    let rustls_config = RustlsServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(certs, key)?;
    Ok(TlsAcceptor::from(Arc::new(rustls_config)))
}

/// Parses one or more certificate blocks from a PEM document.
///
/// # Parameters
///
/// - `pem`: Text containing `CERTIFICATE` PEM blocks.
///
/// # Returns
///
/// DER certificates in the order they appeared in the PEM document.
///
/// # Errors
///
/// Returns an error when no certificate block is present.
fn load_pem_certificates(pem: &str) -> anyhow::Result<Vec<CertificateDer<'static>>> {
    let certs: Vec<_> = pem_blocks(pem, "CERTIFICATE")
        .into_iter()
        .map(CertificateDer::from)
        .collect();
    if certs.is_empty() {
        return Err(anyhow::anyhow!(
            "PEM certificate file contains no certificates"
        ));
    }
    Ok(certs)
}

/// Parses the first supported private key block from a PEM document.
///
/// # Parameters
///
/// - `pem`: Text containing a PKCS#8, RSA, or EC private key PEM block.
///
/// # Returns
///
/// A rustls-owned private key.
///
/// # Errors
///
/// Returns an error when no private key block exists or the decoded key cannot
/// be accepted by rustls.
fn load_pem_private_key(pem: &str) -> anyhow::Result<PrivateKeyDer<'static>> {
    for label in ["PRIVATE KEY", "RSA PRIVATE KEY", "EC PRIVATE KEY"] {
        if let Some(key) = pem_blocks(pem, label).into_iter().next() {
            return PrivateKeyDer::try_from(key)
                .map_err(|error| anyhow::anyhow!("invalid PEM private key: {error}"));
        }
    }
    Err(anyhow::anyhow!(
        "PEM private key file contains no private key"
    ))
}

/// Extracts and decodes PEM blocks matching a label.
///
/// # Parameters
///
/// - `pem`: PEM text to scan.
/// - `label`: PEM block label without the `BEGIN` or `END` markers.
///
/// # Returns
///
/// Decoded DER payloads for blocks that match `label`.
fn pem_blocks(pem: &str, label: &str) -> Vec<Vec<u8>> {
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let mut blocks = Vec::new();
    let mut remaining = pem;

    while let Some(start) = remaining.find(&begin) {
        let after_begin = &remaining[start + begin.len()..];
        let Some(end_offset) = after_begin.find(&end) else {
            break;
        };
        let body = after_begin[..end_offset]
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<String>();
        if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(body) {
            blocks.push(decoded);
        }
        remaining = &after_begin[end_offset + end.len()..];
    }

    blocks
}

#[cfg(test)]
mod tests {
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
}
