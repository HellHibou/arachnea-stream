use crate::core::{ProxyError, Result, UdpAssociation};

/// Placeholder for future MASQUE CONNECT-UDP associations.
pub async fn connect_udp() -> Result<UdpAssociation> {
    Err(ProxyError::Unsupported("masque connect-udp"))
}
