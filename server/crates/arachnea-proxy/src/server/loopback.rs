//! Shared application proxy listener and request-owned routing leases.

use super::access::{generate_api_key, ListenerAccess, ProxyAccess};
use crate::core::{ArachneaProxyCore, ClientContext, ProxyError, Result};
use crate::server::{handlers, ProxyApiKey, ProxyListenerSettings, ServerConfig};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock, Weak};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinHandle;

/// A routing context kept alive by clients and accepted connections, never by the registry.
pub struct LoopbackRoute {
    /// Core used to establish connections for this routing lease.
    pub(crate) core: ArachneaProxyCore,
    /// Default routing parameters associated with this lease.
    pub(crate) context: ClientContext,
    /// Whether connection headers may supply additional routing parameters.
    pub(crate) allow_parameter_headers: bool,
    id: String,
}

impl LoopbackRoute {
    /// Returns the opaque routing identifier; authentication requires the runtime key too.
    pub fn credential(&self) -> &str {
        &self.id
    }
}

struct RunningListener {
    addr: SocketAddr,
    task: JoinHandle<()>,
    access: Arc<ListenerAccess>,
    settings: ProxyListenerSettings,
}
impl Drop for RunningListener {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// One application-owned proxy runtime with replaceable listening generations.
///
/// Authentication and route registries survive rebinds; retired listeners deny
/// new user admissions and drain internal connections for a bounded interval.
pub struct LoopbackProxyRuntime {
    listener: RwLock<Option<RunningListener>>,
    retired: Arc<std::sync::Mutex<Vec<RunningListener>>>,
    executor: std::sync::OnceLock<tokio::runtime::Handle>,
    change: Mutex<()>,
    drain: Arc<tokio::sync::RwLock<()>>,
    access: Arc<ProxyAccess>,
    routes: Arc<RwLock<HashMap<String, Weak<LoopbackRoute>>>>,
    default_route: Arc<RwLock<Weak<LoopbackRoute>>>,
}

impl std::fmt::Debug for LoopbackProxyRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoopbackProxyRuntime")
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
    /// Creates an unbound internal-only runtime with a fresh in-memory API key.
    ///
    /// # Arguments
    /// * `port` - Initial internal port, or zero for OS allocation.
    pub fn new(port: u16) -> Self {
        Self {
            listener: RwLock::new(None),
            retired: Arc::default(),
            executor: std::sync::OnceLock::new(),
            change: Mutex::new(()),
            drain: Arc::default(),
            access: Arc::new(ProxyAccess {
                stopped: AtomicBool::new(false),
                internal_key: generate_api_key(),
                changes: tokio::sync::watch::channel(0).0,
                policy: RwLock::new((
                    ProxyListenerSettings {
                        port,
                        ..Default::default()
                    },
                    Vec::new(),
                )),
            }),
            routes: Arc::default(),
            default_route: Arc::default(),
        }
    }

    /// Returns the active listening address, if started.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.listener
            .read()
            .expect("proxy listener lock poisoned")
            .as_ref()
            .filter(|listener| !listener.task.is_finished())
            .map(|listener| listener.addr)
    }

    /// Returns the active policy without exposing any authentication secrets.
    pub fn settings(&self) -> ProxyListenerSettings {
        self.access
            .policy
            .read()
            .expect("proxy policy lock poisoned")
            .0
            .clone()
    }

    /// Builds internal authentication credentials without putting the key in Debug output.
    ///
    /// # Arguments
    /// * `route` - Registered routing lease owned by the client.
    pub fn internal_credentials(&self, route: &LoopbackRoute) -> (String, String) {
        (
            format!("context={}", route.credential()),
            self.access.internal_key.clone(),
        )
    }

    /// Starts the currently configured listener exactly once.
    ///
    /// # Errors
    /// Returns a contextual bind error or an error after terminal shutdown.
    pub async fn start(&self) -> std::io::Result<SocketAddr> {
        let _guard = self.change.lock().await;
        self.ensure_running()?;
        if let Some(addr) = self.local_addr() {
            return Ok(addr);
        }
        let listener = self.bind(&self.settings()).await?;
        let addr = listener.addr;
        self.ensure_running()?;
        *self.listener.write().expect("proxy listener lock poisoned") = Some(listener);
        Ok(addr)
    }

    /// Retains retired internal listeners while replacement transports are prepared.
    ///
    /// Dropping the returned guard allows their bounded drain timers to begin.
    pub async fn retain_listeners(&self) -> tokio::sync::OwnedRwLockReadGuard<()> {
        self.drain.clone().read_owned().await
    }

    /// Applies a validated policy and key verifiers to the running proxy.
    ///
    /// The old listener survives a failed bind. On success it rejects new user
    /// admissions immediately and drains existing work for a bounded interval.
    ///
    /// # Arguments
    /// * `settings` - Resolved application policy, normalized when disabled.
    /// * `keys` - Persisted cryptographic user-key verifiers.
    ///
    /// # Errors
    /// Returns validation, bind or restoration errors without advertising a failed target.
    pub async fn configure(
        &self,
        settings: ProxyListenerSettings,
        keys: Vec<ProxyApiKey>,
    ) -> std::io::Result<()> {
        settings.validate()?;
        let settings = settings.effective();
        let _guard = self.change.lock().await;
        self.ensure_running()?;
        let previous = self.settings();
        let same_endpoint = self.local_addr().is_some_and(|addr| {
            addr.port() == settings.port
                && addr.ip().is_loopback()
                    == (settings.network
                        == arachnea_core::controler::options::ServerNetworkMode::Local)
        });
        if (previous == settings && self.local_addr().is_some()) || same_endpoint {
            if let Some(listener) = self
                .listener
                .write()
                .expect("proxy listener lock poisoned")
                .as_mut()
            {
                listener.settings = settings.clone();
            }
            *self
                .access
                .policy
                .write()
                .expect("proxy policy lock poisoned") = (settings, keys);
            self.access
                .changes
                .send_modify(|generation| *generation = generation.wrapping_add(1));
            return Ok(());
        }
        let old_addr = self.local_addr();
        // A same-port exposure change cannot bind a second socket portably.
        let same_port = old_addr.is_some_and(|addr| addr.port() == settings.port);
        if same_port {
            let previous = self
                .listener
                .write()
                .expect("proxy listener lock poisoned")
                .take();
            if let Some(mut previous) = previous {
                previous.task.abort();
                let _ = (&mut previous.task).await;
            }
        }
        let reusable = {
            let mut retired = self.retired.lock().expect("proxy retirement lock poisoned");
            retired
                .iter()
                .position(|listener| listener.settings == settings)
                .map(|index| retired.remove(index))
        };
        let prepared = match reusable {
            Some(listener) => Ok(listener),
            None => self.bind(&settings).await,
        };
        let replacement = match prepared {
            Ok(listener) => listener,
            Err(error) => {
                if same_port {
                    let mut restoration = previous.clone();
                    restoration.port = old_addr
                        .expect("same-port replacement has an old address")
                        .port();
                    let mut restored = self.bind(&restoration).await.map_err(|restore| {
                        std::io::Error::new(
                            restore.kind(),
                            format!("Proxy bind failed ({error}); restoration failed ({restore})."),
                        )
                    })?;
                    restored.settings = previous.clone();
                    *self.listener.write().expect("proxy listener lock poisoned") = Some(restored);
                }
                return Err(error);
            }
        };
        self.ensure_running()?;
        replacement.access.external.store(true, Ordering::Release);
        *self
            .access
            .policy
            .write()
            .expect("proxy policy lock poisoned") = (settings, keys);
        let previous = self
            .listener
            .write()
            .expect("proxy listener lock poisoned")
            .replace(replacement);
        if let Some(previous) = previous {
            previous.access.external.store(false, Ordering::Release);
            let id = previous.task.id();
            self.retired
                .lock()
                .expect("proxy retirement lock poisoned")
                .push(previous);
            let retired = Arc::downgrade(&self.retired);
            let drain = self.drain.clone();
            self.executor
                .get_or_init(tokio::runtime::Handle::current)
                .spawn(async move {
                    {
                        let _ready = drain.write().await;
                    }
                    tokio::time::sleep(crate::PROXY_LISTENER_DRAIN).await;
                    if let Some(retired) = retired.upgrade() {
                        retired
                            .lock()
                            .expect("proxy retirement lock poisoned")
                            .retain(|listener| listener.task.id() != id);
                    }
                });
        }
        self.access
            .changes
            .send_modify(|generation| *generation = generation.wrapping_add(1));
        Ok(())
    }

    /// Rejects lifecycle operations after terminal shutdown.
    fn ensure_running(&self) -> std::io::Result<()> {
        if self.access.stopped.load(Ordering::Acquire) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "proxy runtime has stopped",
            ));
        }
        Ok(())
    }

    /// Binds on the application runtime so REST listener replacement cannot stop proxy I/O.
    async fn bind(&self, settings: &ProxyListenerSettings) -> std::io::Result<RunningListener> {
        let ip = if settings.network == arachnea_core::controler::options::ServerNetworkMode::Local
        {
            Ipv4Addr::LOCALHOST
        } else {
            Ipv4Addr::UNSPECIFIED
        };
        let port = settings.port;
        let listener = self
            .executor
            .get_or_init(tokio::runtime::Handle::current)
            .spawn(async move { TcpListener::bind((ip, port)).await })
            .await
            .map_err(std::io::Error::other)?
            .map_err(|error| {
                std::io::Error::new(
                    error.kind(),
                    format!(
                        "Failed to bind application proxy at {ip}:{}: {error}",
                        settings.port
                    ),
                )
            })?;
        let addr = listener.local_addr()?;
        let routes = self.routes.clone();
        let default_route = self.default_route.clone();
        let access = Arc::new(ListenerAccess {
            shared: self.access.clone(),
            external: AtomicBool::new(true),
        });
        let admission = access.clone();
        let config = Arc::new(ServerConfig {
            listen_http: Some(addr),
            listen_socks: None,
            listen_https: None,
            ..ServerConfig::default()
        });
        let task = self
            .executor
            .get_or_init(tokio::runtime::Handle::current)
            .spawn(async move {
                let slots = Arc::new(Semaphore::new(crate::PROXY_LOOPBACK_MAX_CONNECTIONS));
                let mut connections = tokio::task::JoinSet::new();
                loop {
                    let accepted = tokio::select! {
                        accepted = listener.accept() => accepted,
                        _ = connections.join_next(), if !connections.is_empty() => continue,
                    };
                    let (stream, peer) = match accepted {
                        Ok(connection) => connection,
                        Err(error) => {
                            tracing::error!(%error, "application proxy listener failed");
                            break;
                        }
                    };
                    if !admission.allows(peer.ip()) {
                        continue;
                    }
                    let Ok(permit) = slots.clone().try_acquire_owned() else {
                        continue;
                    };
                    let (config, routes, default_route, admission) = (
                        config.clone(),
                        routes.clone(),
                        default_route.clone(),
                        admission.clone(),
                    );
                    connections.spawn(async move {
                        let _permit = permit;
                        let result = async {
                            let mut first = [0];
                            let count = tokio::time::timeout(
                                crate::PROXY_LOOPBACK_INSPECTION_TIMEOUT,
                                stream.peek(&mut first),
                            )
                            .await
                            .map_err(|_| ProxyError::Timeout("proxy protocol detection"))??;
                            if count == 0 {
                                return Ok(());
                            }
                            if first[0] == 5 {
                                handlers::socks::handle_loopback(
                                    stream,
                                    routes,
                                    admission,
                                    peer.ip(),
                                )
                                .await
                            } else {
                                handlers::http::handle_loopback(
                                    stream,
                                    config,
                                    routes,
                                    default_route,
                                    admission,
                                    peer.ip(),
                                )
                                .await
                            }
                        }
                        .await;
                        if let Err(error) = result {
                            tracing::debug!(%peer, %error, "application proxy connection ended");
                        }
                    });
                }
            });
        tracing::info!(%addr, "application proxy listening");
        println!("Proxy Server Application launched: http://{addr}");
        Ok(RunningListener {
            addr,
            task,
            access,
            settings: settings.clone(),
        })
    }

    /// Terminates the runtime; future listener starts and admissions fail.
    pub fn shutdown(&self) {
        self.access.stopped.store(true, Ordering::Release);
        let mut policy = self
            .access
            .policy
            .write()
            .expect("proxy policy lock poisoned");
        policy.0.enabled = false;
        self.listener
            .write()
            .expect("proxy listener lock poisoned")
            .take();
        self.retired
            .lock()
            .expect("proxy retirement lock poisoned")
            .clear();
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
        if routes.len() >= crate::PROXY_LOOPBACK_MAX_ROUTES {
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

    /// Rebinds an exclusively checked-out internal transport before a new request.
    /// Existing tunnels retain their original route; reconnects use the new core.
    ///
    /// # Arguments
    /// - `previous`: Current internal routing lease, with parameter overrides disabled.
    /// - `core`: Newly selected core for the same transport policy and destination.
    ///
    /// # Errors
    /// Returns an error for a stale, external or unavailable routing lease.
    pub fn rebind_internal_route(
        &self,
        previous: &Arc<LoopbackRoute>,
        core: ArachneaProxyCore,
    ) -> Result<Arc<LoopbackRoute>> {
        let mut routes = self
            .routes
            .write()
            .map_err(|_| ProxyError::Config("loopback registry lock poisoned".into()))?;
        if self.access.stopped.load(Ordering::Acquire)
            || previous.allow_parameter_headers
            || !routes
                .get(previous.credential())
                .is_some_and(|route| route.ptr_eq(&Arc::downgrade(previous)))
        {
            return Err(ProxyError::Config(
                "cannot rebind inactive or external loopback route".into(),
            ));
        }
        let route = Arc::new(LoopbackRoute {
            core,
            context: previous.context.clone(),
            allow_parameter_headers: false,
            id: previous.id.clone(),
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
