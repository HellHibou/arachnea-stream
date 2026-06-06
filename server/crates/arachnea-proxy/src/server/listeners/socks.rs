use std::net::SocketAddr;
use std::sync::Arc;

use crate::core::ArachneaProxyCore;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tracing::{debug, warn};

use crate::server::handlers;
use crate::server::ServerConfig;

/// Runs the SOCKS proxy listener.
pub async fn run(
    addr: SocketAddr,
    config: Arc<ServerConfig>,
    core: ArachneaProxyCore,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(%addr, "socks proxy listener started");
    let acl = config.acl();
    let semaphore = Arc::new(Semaphore::new(
        config.network.max_connections.unwrap_or(1024) as usize,
    ));
    loop {
        let (stream, peer) = listener.accept().await?;
        if !acl.allows(peer.ip()) {
            warn!(%peer, "socks client denied by acl");
            continue;
        }
        let permit = Arc::clone(&semaphore).acquire_owned().await?;
        let conn_core = core.clone();
        let conn_config = Arc::clone(&config);
        tokio::spawn(async move {
            let _permit = permit;
            if let Err(error) = handlers::socks::handle(stream, peer, conn_config, conn_core).await
            {
                debug!(%peer, %error, "socks connection ended with error");
            }
        });
    }
}
