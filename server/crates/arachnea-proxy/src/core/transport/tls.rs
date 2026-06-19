use std::sync::Arc;
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, Error, RootCertStore, SignatureScheme};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;

use crate::core::{ProxyError, Result};

/// Logs a TLS client handshake attempt at DEBUG level.
///
/// # Parameters
///
/// - `server_name`: SNI hostname being connected to.
/// - `verify_tls`: Whether certificate verification is enabled.
fn log_client_tls(server_name: &str, verify_tls: bool) {
    tracing::debug!(
        server_name = %server_name,
        verify_tls = %verify_tls,
        transport = "tls",
        "starting tls client handshake"
    );
}

/// Wraps an existing stream in TLS for an HTTPS upstream proxy.
///
/// # Parameters
///
/// - `stream`: Existing stream connected to the HTTPS proxy endpoint.
/// - `server_name`: TLS server name sent through SNI.
/// - `timeout`: Maximum TLS handshake duration.
/// - `verify_tls`: Whether the upstream certificate must validate against
///   public WebPKI roots.
///
/// # Returns
///
/// TLS-protected stream.
///
/// # Errors
///
/// Returns an error when the server name is invalid, the handshake times out,
/// or rustls rejects the connection.
pub async fn client_tls<S>(
    stream: S,
    server_name: &str,
    timeout: Duration,
    verify_tls: bool,
) -> Result<TlsStream<S>>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    log_client_tls(server_name, verify_tls);
    let config = if verify_tls {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|error| ProxyError::Tls(error.to_string()))?
            .with_root_certificates(roots)
            .with_no_client_auth()
    } else {
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|error| ProxyError::Tls(error.to_string()))?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoCertificateVerification))
            .with_no_client_auth()
    };
    let connector = TlsConnector::from(Arc::new(config));
    let server_name = ServerName::try_from(server_name.to_string())
        .map_err(|_| ProxyError::Tls("invalid upstream tls server name".to_string()))?;
    time::timeout(timeout, connector.connect(server_name, stream))
        .await
        .map_err(|_| ProxyError::Timeout("https proxy tls handshake"))?
        .map_err(|error| ProxyError::Tls(error.to_string()))
}

/// Certificate verifier used only when a node explicitly disables TLS checks.
#[derive(Debug)]
struct NoCertificateVerification;

impl ServerCertVerifier for NoCertificateVerification {
    /// Accepts any server certificate chain.
    ///
    /// # Parameters
    ///
    /// - `_end_entity`: End-entity certificate supplied by the server.
    /// - `_intermediates`: Intermediate certificates supplied by the server.
    /// - `_server_name`: Server name requested by the client.
    /// - `_ocsp_response`: Stapled OCSP response bytes.
    /// - `_now`: Current time supplied by rustls.
    ///
    /// # Returns
    ///
    /// Verification marker accepted by rustls.
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, Error> {
        Ok(ServerCertVerified::assertion())
    }

    /// Accepts TLS 1.2 handshake signatures.
    ///
    /// # Parameters
    ///
    /// - `_message`: Handshake message signed by the peer.
    /// - `_cert`: Certificate associated with the signature.
    /// - `_dss`: Digitally-signed structure from rustls.
    ///
    /// # Returns
    ///
    /// Signature verification marker accepted by rustls.
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    /// Accepts TLS 1.3 handshake signatures.
    ///
    /// # Parameters
    ///
    /// - `_message`: Handshake message signed by the peer.
    /// - `_cert`: Certificate associated with the signature.
    /// - `_dss`: Digitally-signed structure from rustls.
    ///
    /// # Returns
    ///
    /// Signature verification marker accepted by rustls.
    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    /// Returns signature schemes advertised to the peer.
    ///
    /// # Returns
    ///
    /// Signature algorithms accepted by this verifier.
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::ED25519,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
        ]
    }
}
