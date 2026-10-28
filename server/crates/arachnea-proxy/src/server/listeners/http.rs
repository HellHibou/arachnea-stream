use std::net::SocketAddr;
use std::sync::Arc;

use crate::core::ArachneaProxyCore;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tracing::{debug, warn};

use crate::server::handlers;
use crate::server::ServerConfig;

/// Runs the HTTP proxy listener.
///
/// # Arguments
/// * `addr` - Local socket address to bind for incoming proxy connections.
/// * `config` - Shared server configuration controlling limits, ACLs and authentication.
/// * `core` - Proxy core used to route and establish outbound connections.
///
/// # Errors
///
/// Returns an error if binding, accepting a connection or acquiring a listener permit fails. Individual connection failures are logged.
pub async fn run(
    addr: SocketAddr,
    config: Arc<ServerConfig>,
    core: ArachneaProxyCore,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(%addr, "http proxy listener started");
    let acl = config.acl();
    let semaphore = Arc::new(Semaphore::new(
        config.network.max_connections.unwrap_or(1024) as usize,
    ));
    loop {
        let (stream, peer) = listener.accept().await?;
        if !acl.allows(peer.ip()) {
            warn!(%peer, "http proxy client denied by acl");
            continue;
        }
        let permit = Arc::clone(&semaphore).acquire_owned().await?;
        let conn_config = Arc::clone(&config);
        let conn_core = core.clone();
        tokio::spawn(async move {
            let _permit = permit;
            if let Err(error) = handlers::http::handle(stream, peer, conn_config, conn_core).await {
                debug!(%peer, %error, "http proxy connection ended with error");
            }
        });
    }
}
