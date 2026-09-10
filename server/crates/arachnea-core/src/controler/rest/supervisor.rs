//! Hot application of REST server settings (port, network mode, entry root).
//!
//! The supervisor owns the TCP listener and the accept loop of the REST
//! server. Instead of freezing the warp router at bind time, each accepted
//! connection is served with the current snapshot of the router, which makes
//! it possible to replay the service registrations without dropping the
//! listener. When one of the three dynamic settings (port, network mode,
//! entrypoint root) actually changes, the supervisor stops the current accept
//! loop gracefully, rebuilds a fresh [`RestControlerService`] by replaying the
//! recorded registration steps, and re-binds the new socket address, rolling
//! back to the previous configuration when the re-bind fails.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::watch;

use super::service::{ReplayStep, RestControlerService, RestServiceSnapshot};
use super::tray::{ServerTrayHandle, ServerTrayUpdate};
use super::{build_public_url, display_server_host, local_networks, RestRouter};
use crate::controler::main_thread::QueuedMainThreadDispatcher;
use crate::controler::rest::shutdown::ShutdownSignal;
use crate::controler::ServerNetworkMode;

/// HTTP header injected by the accept loop to propagate the peer address.
///
/// `warp::service` drops the transport-level remote address, so the accept
/// loop forwards it through this header. Incoming values are stripped before
/// the header is (re)injected, which makes spoofing impossible from clients.
pub(crate) const PEER_ADDR_HEADER: &str = "x-arachnea-peer-addr";

/// Maximum time the supervisor waits for in-flight connections to complete
/// when stopping the current accept loop.
const ACCEPT_DRAIN: Duration = Duration::from_secs(2);

/// Maximum time the supervisor waits for the accept-loop thread to finish
/// after shutdown has been requested.
const STOP_GRACE: Duration = Duration::from_secs(3);

/// Target dynamic settings of the REST server, resolved by the application
/// crate (command-line overrides combined with the persisted configuration).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestServerSettings {
    /// Effective HTTP port.
    pub server_port: u16,
    /// Effective network access mode.
    pub network_mode: ServerNetworkMode,
    /// Effective public root path, when set.
    pub entrypoint_root: Option<String>,
}

/// Report of a hot application attempt of the REST server settings.
#[derive(Clone, Debug)]
pub struct RestServerApplyReport {
    /// Whether the target settings are now active on the running server.
    pub applied: bool,
    /// Failure context when the settings could not be applied.
    pub apply_error: Option<String>,
    /// Effective port after the application attempt.
    pub server_port: u16,
    /// Effective network mode after the application attempt.
    pub network_mode: String,
    /// Effective entrypoint root after the application attempt.
    pub entrypoint_root: Option<String>,
    /// Public URL of the running server.
    pub server_url: String,
    /// Public URL of the administration interface.
    pub admin_url: String,
}

/// Application-supplied resolver of the target REST server settings.
///
/// Returning `None` signals that hot application is not available (for
/// example in desktop mode), leaving the running server untouched.
pub type RestSettingsSource = Arc<dyn Fn() -> Option<RestServerSettings> + Send + Sync>;

/// Shared handle letting the application trigger a hot application of the
/// REST server settings (port, network mode, entrypoint root).
pub struct RestServerHandle {
    supervisor: Arc<RestServerSupervisor>,
}

impl RestServerHandle {
    /// Wraps a supervisor into a shared application handle.
    pub(crate) fn for_supervisor(supervisor: Arc<RestServerSupervisor>) -> Self {
        Self { supervisor }
    }

    /// Sets the resolver used to compute the target settings at apply time.
    pub fn set_settings_source(&self, source: RestSettingsSource) {
        self.supervisor.set_settings_source(source);
    }

    /// Resolves the current target settings (command-line overrides applied on
    /// top of the persisted configuration), when the source is available.
    pub fn target_settings(&self) -> Option<RestServerSettings> {
        self.supervisor.resolve_target()
    }

    /// Applies the current target settings to the running server.
    ///
    /// Blocking call: it stops the current accept loop when a socket change is
    /// required, rebuilds the router, and re-binds the listener. The returned
    /// report always reflects the configuration actually active afterwards.
    pub fn apply_settings(&self) -> RestServerApplyReport {
        self.supervisor.apply()
    }
}

/// Runtime state of the supervised REST server.
struct RuntimeState {
    /// Dynamic settings currently applied to the running server.
    settings: RestServerSettings,
    /// Socket address currently bound by the running server.
    socket_addr: SocketAddr,
    /// Networks currently allowed by the ACL filter.
    allowed_networks: Vec<ipnet::IpNet>,
    /// Shutdown signal of the current accept loop.
    shutdown: Option<ShutdownSignal>,
    /// Handle of the current accept-loop thread.
    server_thread: Option<std::thread::JoinHandle<()>>,
    /// Sender of the current router snapshot.
    router_tx: Option<watch::Sender<RestRouter>>,
    /// Critical failure state: the server is down and every re-bind attempt
    /// failed. Kept until a later apply succeeds.
    failed: Option<String>,
}

/// Supervisor owning the REST server lifecycle and its hot reconfiguration.
pub(crate) struct RestServerSupervisor {
    /// Original configuration pieces used to rebuild service instances.
    base: RestServiceSnapshot,
    /// Replayed registration steps recorded from the service registrations.
    steps: Mutex<Vec<ReplayStep>>,
    /// Main-thread dispatcher shared by every rebuilt service instance.
    dispatcher: Arc<QueuedMainThreadDispatcher>,
    /// Application-supplied resolver of the target settings.
    settings_source: Mutex<Option<RestSettingsSource>>,
    /// Current runtime state of the supervised server.
    runtime: Mutex<RuntimeState>,
    /// Guards concurrent apply requests.
    apply_lock: Mutex<()>,
    /// Process-level shutdown signal, distinct from the per-instance accept
    /// loop signal so a hot re-bind does not stop the main loop or the tray.
    app_shutdown: Mutex<Option<ShutdownSignal>>,
    /// Slot receiving the live tray handle, filled either synchronously by the
    /// tray factory or during its setup (macOS blocks in `run`).
    tray_sink: Arc<std::sync::OnceLock<Arc<dyn ServerTrayHandle>>>,
}

impl RestServerSupervisor {
    /// Creates the supervisor from the initial service configuration.
    pub(crate) fn new(
        base: RestServiceSnapshot,
        dispatcher: Arc<QueuedMainThreadDispatcher>,
    ) -> Self {
        let settings = base.initial_settings();
        let socket_addr = base.socket_addr();
        let allowed_networks = base.allowed_networks.clone();
        Self {
            base,
            steps: Mutex::new(Vec::new()),
            dispatcher,
            settings_source: Mutex::new(None),
            runtime: Mutex::new(RuntimeState {
                settings,
                socket_addr,
                allowed_networks,
                shutdown: None,
                server_thread: None,
                router_tx: None,
                failed: None,
            }),
            apply_lock: Mutex::new(()),
            app_shutdown: Mutex::new(None),
            tray_sink: Arc::new(std::sync::OnceLock::new()),
        }
    }

    /// Records a registration step replayed when the router is rebuilt.
    pub(crate) fn record_step(&self, step: ReplayStep) {
        self.steps
            .lock()
            .expect("supervisor steps poisoned")
            .push(step);
    }

    /// Sets the resolver used to compute the target settings.
    pub(crate) fn set_settings_source(&self, source: RestSettingsSource) {
        *self
            .settings_source
            .lock()
            .expect("supervisor settings source poisoned") = Some(source);
    }

    /// Resolves the target settings with the same normalization as `apply`.
    pub(crate) fn resolve_target(&self) -> Option<RestServerSettings> {
        let source = self
            .settings_source
            .lock()
            .expect("supervisor settings source poisoned")
            .clone()?;
        let target = source()?;
        Some(normalize_target(target))
    }

    /// Returns the slot receiving the live tray handle.
    pub(crate) fn tray_handle_sink(&self) -> Arc<std::sync::OnceLock<Arc<dyn ServerTrayHandle>>> {
        Arc::clone(&self.tray_sink)
    }

    /// Waits for the running accept-loop thread to finish.
    ///
    /// Bounded by [`STOP_GRACE`]; after that the thread is abandoned (it would
    /// still be cut when the process exits). Used by `launch()` after the
    /// process-level shutdown has been requested so the HTTP server finishes
    /// its graceful stop before the launch call returns.
    pub(crate) fn join_server(&self) {
        let thread = {
            let mut runtime = self.runtime.lock().expect("supervisor runtime poisoned");
            runtime.server_thread.take()
        };
        if let Some(thread) = thread {
            wait_for_thread(thread, STOP_GRACE);
        }
    }

    /// Starts the accept loop for the initial router and socket address.
    ///
    /// Returns the process-level shutdown signal (shared with the tray so its
    /// "close" action stops the whole server). It is distinct from the
    /// per-instance signal driving the accept loop, so hot re-binds do not
    /// stop the application.
    ///
    /// Binding failures keep the historical panicking behaviour of the first
    /// launch: the process cannot serve without its REST server.
    pub(crate) fn launch_controller(
        &self,
        router: RestRouter,
        socket_addr: SocketAddr,
        entrypoint_root: &[String],
    ) -> ShutdownSignal {
        let listener = TcpListener::bind(socket_addr).unwrap_or_else(|error| {
            panic!("Failed to bind the REST server on {socket_addr}: {error}")
        });
        let shutdown = ShutdownSignal::new();
        let (router_tx, router_rx) = watch::channel(router);

        let app_shutdown = ShutdownSignal::new();
        *self
            .app_shutdown
            .lock()
            .expect("supervisor app shutdown poisoned") = Some(app_shutdown.clone());

        let server_thread =
            spawn_accept_thread(listener, router_rx, shutdown.clone(), app_shutdown.clone());

        let prefix = if entrypoint_root.is_empty() {
            "/".to_string()
        } else {
            format!("/{}/", entrypoint_root.join("/"))
        };
        println!(
            "Web Server Application launched: http://{}:{}{}",
            display_server_host(socket_addr),
            socket_addr.port(),
            prefix
        );

        let mut runtime = self.runtime.lock().expect("supervisor runtime poisoned");
        runtime.shutdown = Some(shutdown);
        runtime.server_thread = Some(server_thread);
        runtime.router_tx = Some(router_tx);
        app_shutdown
    }

    /// Applies the target settings to the running server.
    ///
    /// The full cycle (stop, rebuild, re-bind) only runs when the bound address
    /// changes. ACL and root changes swap the router on the existing listener;
    /// each request resolves the latest router snapshot.
    pub(crate) fn apply(&self) -> RestServerApplyReport {
        let _apply_guard = self
            .apply_lock
            .lock()
            .expect("supervisor apply lock poisoned");

        let source = self
            .settings_source
            .lock()
            .expect("supervisor settings source poisoned")
            .clone();
        let Some(source) = source else {
            return self.report(false, Some("settings source unavailable".to_string()));
        };
        // Normalize the target so textual variants of "no prefix" (`""`, `/`)
        // compare equal to an absent root.
        let Some(target) = source().map(normalize_target) else {
            return self.report(false, Some("settings source unavailable".to_string()));
        };

        let mut runtime = self.runtime.lock().expect("supervisor runtime poisoned");
        let target_snapshot = snapshot_for(&self.base, &target);
        let needs_rebind = target_snapshot.socket_addr() != runtime.socket_addr;

        if !needs_rebind && runtime.failed.is_none() {
            // Same-listener replay: rebuild the router and swap the snapshot
            // served to newly accepted connections.
            let router = self.build_router(&target_snapshot);
            if let Some(router_tx) = &runtime.router_tx {
                if router_tx.send(router).is_err() {
                    drop(runtime);
                    return self.report(false, Some("router snapshot channel closed".to_string()));
                }
            }
            runtime.settings = target.clone();
            runtime.allowed_networks = target_snapshot.allowed_networks.clone();
            drop(runtime);
            self.update_tray(&target_snapshot);
            return self.report(true, None);
        }

        let target_addr = target_snapshot.socket_addr();
        // Pre-check the target bind so a busy port is detected before the
        // running server is stopped. This shrinks (without removing) the
        // window where the port would be left unbound.
        if let Err(error) = TcpListener::bind(target_addr) {
            drop(runtime);
            return self.report(false, Some(format!("cannot bind {target_addr}: {error}")));
        }

        // Stop the current server gracefully (bounded).
        if let Some(shutdown) = &runtime.shutdown {
            shutdown.request();
        }
        if let Some(thread) = runtime.server_thread.take() {
            wait_for_thread(thread, STOP_GRACE);
        }

        match self.start_service(&mut runtime, &target_snapshot, &target) {
            Ok(()) => {
                runtime.failed = None;
                drop(runtime);
                self.update_tray(&target_snapshot);
                self.report(true, None)
            }
            Err(target_error) => {
                // Roll back to the previously applied configuration so the
                // server stays reachable with consistent settings.
                let previous = runtime.settings.clone();
                let previous_snapshot = snapshot_for(&self.base, &previous);
                match self.start_service(&mut runtime, &previous_snapshot, &previous) {
                    Ok(()) => {
                        runtime.failed = None;
                        drop(runtime);
                        self.update_tray(&previous_snapshot);
                        self.report(
                            false,
                            Some(format!(
                                "failed to apply settings ({target_error}); rolled back to the previous configuration"
                            )),
                        )
                    }
                    Err(previous_error) => {
                        let failure = format!(
                            "failed to apply settings ({target_error}) and rollback failed ({previous_error}); the HTTP server is down"
                        );
                        runtime.failed = Some(failure.clone());
                        drop(runtime);
                        self.report(false, Some(failure))
                    }
                }
            }
        }
    }

    /// Builds a fresh router by replaying the recorded registration steps.
    pub(crate) fn build_router(&self, snapshot: &RestServiceSnapshot) -> RestRouter {
        let mut service =
            RestControlerService::from_snapshot(snapshot.clone(), Arc::clone(&self.dispatcher));
        // Fresh HTML generation for this server (re)start: `replace_html_base`
        // output depends on the runtime `entrypoint_root`, so HTML documents
        // must never validate against a previous generation.
        service.set_web_generation(crate::controler::web_assets::new_web_generation());
        {
            let steps = self.steps.lock().expect("supervisor steps poisoned");
            for step in steps.iter() {
                step(&mut service);
            }
        }
        service.finalize_router()
    }

    /// Starts a new accept loop for the given snapshot and records it as the
    /// active runtime state.
    fn start_service(
        &self,
        runtime: &mut RuntimeState,
        snapshot: &RestServiceSnapshot,
        settings: &RestServerSettings,
    ) -> Result<(), String> {
        let addr = snapshot.socket_addr();
        let router = self.build_router(snapshot);
        let listener =
            TcpListener::bind(addr).map_err(|error| format!("cannot bind {addr}: {error}"))?;
        let shutdown = ShutdownSignal::new();
        let (router_tx, router_rx) = watch::channel(router);
        // Every re-bind shares the same process-level shutdown so the tray
        // "close" action stops the currently running accept loop too.
        let app_shutdown = self
            .app_shutdown
            .lock()
            .expect("supervisor app shutdown poisoned")
            .clone()
            .expect("REST supervisor re-bind before launch");
        let server_thread =
            spawn_accept_thread(listener, router_rx, shutdown.clone(), app_shutdown);

        runtime.settings = settings.clone();
        runtime.socket_addr = addr;
        runtime.allowed_networks = snapshot.allowed_networks.clone();
        runtime.shutdown = Some(shutdown);
        runtime.server_thread = Some(server_thread);
        runtime.router_tx = Some(router_tx);
        Ok(())
    }

    /// Updates the systray with the configuration actually active.
    fn update_tray(&self, snapshot: &RestServiceSnapshot) {
        let Some(handle) = self.tray_sink.get() else {
            return;
        };
        let socket_addr = snapshot.socket_addr();
        let root = snapshot.root_segments();
        let mut admin_url = build_public_url(socket_addr, &root);
        if !admin_url.ends_with('/') {
            admin_url.push('/');
        }
        admin_url.push_str("admin/");
        let server_prefix = if root.is_empty() {
            "/".to_string()
        } else {
            format!("/{}/", root.join("/"))
        };
        let network_mode = if socket_addr.ip().is_loopback() {
            "Local".to_string()
        } else if snapshot.allowed_networks.is_empty() {
            "Public".to_string()
        } else {
            "Private".to_string()
        };
        handle.update_configuration(&ServerTrayUpdate {
            server_url: build_public_url(socket_addr, &root),
            admin_url,
            server_display_url: format!(
                "http://{}:{}{}",
                display_server_host(socket_addr),
                socket_addr.port(),
                server_prefix
            ),
            network_mode,
        });
    }

    /// Builds the report for the configuration currently active.
    fn report(&self, applied: bool, apply_error: Option<String>) -> RestServerApplyReport {
        let runtime = self.runtime.lock().expect("supervisor runtime poisoned");
        let snapshot = snapshot_for(&self.base, &runtime.settings);
        let root = snapshot.root_segments();
        let mut admin_url = build_public_url(runtime.socket_addr, &root);
        if !admin_url.ends_with('/') {
            admin_url.push('/');
        }
        admin_url.push_str("admin/");
        RestServerApplyReport {
            applied,
            apply_error,
            server_port: runtime.settings.server_port,
            network_mode: runtime.settings.network_mode.to_string(),
            entrypoint_root: runtime.settings.entrypoint_root.clone(),
            server_url: build_public_url(runtime.socket_addr, &root),
            admin_url,
        }
    }
}

/// Normalizes a target so textual variants of "no prefix" (`""`, `/`)
/// compare equal to an absent root.
fn normalize_target(mut target: RestServerSettings) -> RestServerSettings {
    if target.entrypoint_root.as_deref().map(str::is_empty) == Some(true) {
        target.entrypoint_root = None;
    }
    target
}

/// Builds the service snapshot matching the given dynamic settings.
fn snapshot_for(base: &RestServiceSnapshot, settings: &RestServerSettings) -> RestServiceSnapshot {
    let mut snapshot = base.clone();
    snapshot.server_port = settings.server_port;
    // Normalize an empty root to `None` so textual variants (`""`, `/`) of the
    // same "no prefix" configuration compare equal.
    snapshot.entrypoint_root = settings
        .entrypoint_root
        .as_deref()
        .filter(|root| !root.is_empty())
        .map(ToString::to_string);
    match settings.network_mode {
        ServerNetworkMode::Local => {
            snapshot.server_ip = None;
            snapshot.allowed_networks = Vec::new();
        }
        ServerNetworkMode::Private => {
            snapshot.server_ip = Some(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
            snapshot.allowed_networks = local_networks();
        }
        ServerNetworkMode::Public => {
            snapshot.server_ip = Some(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
            snapshot.allowed_networks = Vec::new();
        }
    }
    snapshot
}

/// Waits for a thread to finish up to the given timeout, then abandons it.
fn wait_for_thread(thread: std::thread::JoinHandle<()>, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if thread.is_finished() {
            let _ = thread.join();
            return;
        }
        if Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Spawns the dedicated thread running the accept loop on its own runtime.
///
/// The loop stops when the per-instance shutdown is requested (hot re-bind)
/// **or** when the process-level shutdown is requested (tray "close").
/// Each request resolves the latest router snapshot. Requests already executing
/// finish under the snapshot selected when they started.
fn spawn_accept_thread(
    listener: TcpListener,
    router_rx: watch::Receiver<RestRouter>,
    shutdown: ShutdownSignal,
    app_shutdown: ShutdownSignal,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("Failed to create the REST server runtime.");
        runtime.block_on(async move {
            let _ = listener.set_nonblocking(true);
            let listener = match tokio::net::TcpListener::from_std(listener) {
                Ok(listener) => listener,
                Err(error) => {
                    tracing::error!(error = %error, "failed to adopt the REST listener");
                    return;
                }
            };
            let mut connections: Vec<tokio::task::JoinHandle<()>> = Vec::new();
            loop {
                tokio::select! {
                    _ = shutdown.wait() => break,
                    _ = app_shutdown.wait() => break,
                    accepted = listener.accept() => {
                        match accepted {
                            Ok((stream, _peer)) => {
                                connections.retain(|task| !task.is_finished());
                                let router_rx = router_rx.clone();
                                connections
                                    .push(tokio::task::spawn(serve_connection(stream, router_rx)));
                            }
                            Err(error) => {
                                tracing::warn!(error = %error, "REST accept failed");
                                tokio::time::sleep(Duration::from_millis(50)).await;
                            }
                        }
                    }
                }
            }
            // Bounded drain: in-flight requests may complete, streams still
            // running after the deadline are cut with the runtime shutdown.
            let _ =
                tokio::time::timeout(ACCEPT_DRAIN, futures::future::join_all(connections)).await;
        });
        runtime.shutdown_timeout(Duration::from_millis(500));
    })
}

/// Serves one HTTP connection using the router current for each request.
async fn serve_connection(stream: tokio::net::TcpStream, router_rx: watch::Receiver<RestRouter>) {
    use warp::hyper::service::Service as _;
    let peer = stream.peer_addr().ok();
    let _ = warp::hyper::server::conn::Http::new()
        .serve_connection(
            stream,
            warp::hyper::service::service_fn(move |mut request| {
                // Propagate the peer address through a controlled header so
                // route filters can read it (`warp::service` drops the
                // transport-level address). Incoming values are stripped,
                // making client spoofing impossible.
                use warp::hyper::header::HeaderValue;
                request.headers_mut().remove(PEER_ADDR_HEADER);
                if let Some(peer) = peer {
                    if let Ok(value) = HeaderValue::from_str(&peer.to_string()) {
                        request.headers_mut().insert(PEER_ADDR_HEADER, value);
                    }
                }
                let mut service = warp::service(router_rx.borrow().clone());
                async move { service.call(request).await }
            }),
        )
        .await;
}
