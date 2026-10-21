//! Shared HTTP/internal SOCKS5 loopback listener and request-owned routing leases.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock, Weak};

use tokio::net::TcpListener;
use tokio::sync::{OnceCell, Semaphore};
use tokio::task::JoinHandle;

use crate::core::{ArachneaProxyCore, ClientContext, ProxyError, Result};
use crate::server::{handlers, ServerConfig};

/// A routing context kept alive by clients and accepted connections, never by the registry.
pub struct LoopbackRoute {
    pub(crate) core: ArachneaProxyCore,
    pub(crate) context: ClientContext,
    pub(crate) allow_parameter_headers: bool,
    id: String,
}

impl LoopbackRoute {
    /// Returns the opaque credential used to select this context on the shared listener.
    pub fn credential(&self) -> &str {
        &self.id
    }
}

struct RunningListener {
    addr: SocketAddr,
    task: JoinHandle<()>,
}

impl Drop for RunningListener {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// One lazily bound loopback listener shared across an application runtime.
///
/// The registry and default route contain weak references to avoid ownership
/// cycles through provider clients. Keep the default route lease alive in the
/// application owner. Dropping this runtime terminates accepted connections.
pub struct LoopbackProxyRuntime {
    port: u16,
    listener: OnceCell<RunningListener>,
    routes: Arc<RwLock<HashMap<String, Weak<LoopbackRoute>>>>,
    default_route: Arc<RwLock<Weak<LoopbackRoute>>>,
    stopped: AtomicBool,
}

impl std::fmt::Debug for LoopbackProxyRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoopbackProxyRuntime")
            .field("port", &self.port)
            .field("address", &self.local_addr())
            .finish_non_exhaustive()
    }
}

impl Default for LoopbackProxyRuntime {
    fn default() -> Self {
        Self::new(crate::PROXY_LOOPBACK_DEFAULT_PORT)
    }
}

impl LoopbackProxyRuntime {
    /// Creates an unbound runtime restricted to IPv4 loopback.
    ///
    /// # Arguments
    /// - `port`: Required listening port, or zero for OS allocation.
    pub fn new(port: u16) -> Self {
        Self {
            port,
            listener: OnceCell::new(),
            routes: Arc::default(),
            default_route: Arc::default(),
            stopped: AtomicBool::new(false),
        }
    }

    /// Returns the bound address, if the listener has started.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.listener.get().map(|listener| listener.addr)
    }

    /// Reserves the configured socket once and starts accepting connections.
    ///
    /// # Errors
    /// Returns an I/O error if binding fails or the runtime has stopped.
    /// A fixed port never falls back to another port.
    pub async fn start(&self) -> std::io::Result<SocketAddr> {
        if self.stopped.load(Ordering::Acquire) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "loopback proxy runtime has stopped",
            ));
        }
        let running = self.listener.get_or_try_init(|| async {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, self.port)).await.map_err(|error| {
                std::io::Error::new(error.kind(), format!("failed to bind loopback proxy at 127.0.0.1:{}: {error}", self.port))
            })?;
            let addr = listener.local_addr()?;
            let routes = self.routes.clone();
            let default_route = self.default_route.clone();
            let config = Arc::new(ServerConfig {
                listen_http: Some(addr),
                listen_socks: None,
                listen_https: None,
                ..ServerConfig::default()
            });
            let task = tokio::spawn(async move {
                let slots = Arc::new(Semaphore::new(1024));
                let mut connections = tokio::task::JoinSet::new();
                loop {
                    let accepted = tokio::select! {
                        accepted = listener.accept() => accepted,
                        _ = connections.join_next(), if !connections.is_empty() => continue,
                    };
                    let (stream, peer) = match accepted {
                        Ok(connection) => connection,
                        Err(error) => {
                            tracing::error!(%error, "shared loopback proxy listener failed");
                            break;
                        }
                    };
                    if !peer.ip().is_loopback() || !config.acl().allows(peer.ip()) {
                        continue;
                    }
                    let Ok(permit) = slots.clone().try_acquire_owned() else { continue; };
                    let config = config.clone();
                    let routes = routes.clone();
                    let default_route = default_route.clone();
                    connections.spawn(async move {
                        let _permit = permit;
                        let result = async {
                            let mut first = [0];
                            let count = tokio::time::timeout(std::time::Duration::from_secs(30), stream.peek(&mut first))
                                .await.map_err(|_| ProxyError::Timeout("loopback protocol detection"))??;
                            if count == 0 { return Ok(()); }
                            if first[0] == 5 {
                                handlers::socks::handle_loopback(stream, routes).await
                            } else {
                                handlers::http::handle_loopback(stream, config, routes, default_route).await
                            }
                        }.await;
                        if let Err(error) = result {
                            tracing::debug!(%peer, %error, "shared loopback proxy connection ended");
                        }
                    });
                }
            });
            tracing::info!(%addr, automatic_port = self.port == 0, "shared loopback proxy listening");
            println!("Proxy Server Application launched: http://{addr}");
            Ok::<_, std::io::Error>(RunningListener { addr, task })
        }).await?;
        if self.stopped.load(Ordering::Acquire) {
            running.task.abort();
        }
        if self.stopped.load(Ordering::Acquire) || running.task.is_finished() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "loopback proxy listener has stopped",
            ));
        }
        Ok(running.addr)
    }

    /// Stops acceptance and all tracked connections, even if clients retain routing leases.
    ///
    /// Shutdown is terminal; this runtime cannot be rebound or restarted.
    pub fn shutdown(&self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(listener) = self.listener.get() {
            listener.task.abort();
        }
    }

    /// Registers a context until its last client or connection releases the lease.
    ///
    /// # Arguments
    /// - `core`: General or request-pinned routing core.
    /// - `context`: Parameters applied to connections using this lease.
    /// - `allow_parameter_headers`: Whether clients may supply additional proxy parameters.
    ///
    /// # Errors
    /// Returns an error when the registry is poisoned or its live limit is reached.
    pub fn register(
        &self,
        core: ArachneaProxyCore,
        context: ClientContext,
        allow_parameter_headers: bool,
    ) -> Result<Arc<LoopbackRoute>> {
        let mut routes = self
            .routes
            .write()
            .map_err(|_| ProxyError::Config("loopback registry lock poisoned".into()))?;
        routes.retain(|_, route| route.strong_count() > 0);
        if routes.len() >= 8192 {
            return Err(ProxyError::Config(
                "loopback routing context limit reached".into(),
            ));
        }
        let id = format!("{:032x}", rand::random::<u128>());
        let route = Arc::new(LoopbackRoute {
            core,
            context,
            allow_parameter_headers,
            id,
        });
        routes.insert(route.id.clone(), Arc::downgrade(&route));
        Ok(route)
    }

    /// Publishes the route for external clients, retaining no strong reference to it.
    ///
    /// # Arguments
    /// - `route`: Lease owned by the current application configuration.
    ///
    /// # Errors
    /// Returns an error if the default-route lock is poisoned.
    pub fn set_default_route(&self, route: &Arc<LoopbackRoute>) -> Result<()> {
        *self
            .default_route
            .write()
            .map_err(|_| ProxyError::Config("loopback default route lock poisoned".into()))? =
            Arc::downgrade(route);
        Ok(())
    }

    /// Withdraws a default route only if it still belongs to this lease.
    ///
    /// # Arguments
    /// - `route`: Previously published lease; a newer application generation is preserved.
    ///
    /// # Errors
    /// Returns an error if the default-route lock is poisoned.
    pub fn clear_default_route(&self, route: &Arc<LoopbackRoute>) -> Result<()> {
        let mut current = self
            .default_route
            .write()
            .map_err(|_| ProxyError::Config("loopback default route lock poisoned".into()))?;
        if current.ptr_eq(&Arc::downgrade(route)) {
            *current = Weak::new();
        }
        Ok(())
    }
}
