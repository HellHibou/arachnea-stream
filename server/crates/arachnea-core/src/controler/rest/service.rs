//! REST controller service based on Warp.

use std::sync::Arc;

use serde_json::Value;
use warp::filters::BoxedFilter;
use warp::http::StatusCode;
use warp::{reply, Filter, Rejection, Reply};

use crate::controler::web_assets::{
    asset_etag_for, etag_not_modified, new_web_generation, normalize_mount_path,
    replace_html_base, scope_web_asset_source, web_instance_etag, WebAssetSource,
};
use crate::controler::{
    install_global_main_thread_dispatcher, main_thread::MainThreadDispatchLoop,
    main_thread::QueuedMainThreadDispatcher, ControlerFunctionInput, ControlerJsonInput,
    ControlerJsonOutput, ControlerService, ControlerStreamInput, ControlerStreamOutput,
    JsonControlerFunction, MainThreadDispatcher, RequestControlerContext, ResponseBody,
    SerializedControlerFunction, SharedWebAssets, StreamControlerFunction,
};

use super::configuration::RestControlerConfiguration;
#[cfg(target_os = "macos")]
use super::shutdown::ShutdownSignal;
use super::supervisor::{RestServerHandle, RestServerSettings, RestServerSupervisor};
use super::tray::{gui_available, ServerTrayConfiguration, ServerTrayFactory};
use super::{build_public_url, display_server_host, RestReply, RestRouter};
use crate::controler::ServerNetworkMode;

/// REST controller service.
///
/// This backend exposes registered commands over HTTP using JSON payloads.
pub struct RestControlerService {
    /// Socket address the HTTP server is bound to.
    socket_addr: std::net::SocketAddr,
    /// Warp router containing all registered routes.
    router: Option<RestRouter>,
    /// Networks whose clients are allowed to connect to the server.
    ///
    /// When non-empty, the server rejects requests from addresses outside these
    /// networks. This powers `ServerNetworkMode::Private`.
    allowed_networks: Vec<ipnet::IpNet>,
    /// Parsed entry point root path segments.
    entrypoint_root: Vec<String>,
    /// API path segment mounted under the root prefix.
    entrypoint_api: Option<String>,
    /// Dispatcher for executing commands on the main thread.
    main_thread_dispatcher: Arc<QueuedMainThreadDispatcher>,
    /// Main thread dispatch loop handle.
    main_thread_loop: Option<MainThreadDispatchLoop>,
    /// Tray factory supplied by the application crate.
    tray_factory: Option<Arc<dyn ServerTrayFactory>>,
    /// Whether the server tray should be shown when a GUI is available.
    tray_enabled: bool,
    /// Callback reloading the application configuration from the tray.
    reload_configuration: Option<Arc<dyn Fn() -> String + Send + Sync>>,
    /// Supervisor owning the server lifecycle and its hot reconfiguration.
    ///
    /// `None` on rebuilt instances created by the supervisor itself.
    supervisor: Option<Arc<RestServerSupervisor>>,
    /// Whether registration calls are recorded as replayable steps.
    recording: bool,
    /// Server generation ETag (`T:`) for HTML documents of this router build.
    ///
    /// Regenerated on every server (re)start (fresh process or supervisor
    /// re-bind) because `replace_html_base` output depends on the runtime
    /// `entrypoint_root`. Set before replaying registration steps so rebuilt
    /// filters capture the current generation, never a stale one.
    web_generation: String,
}

/// Registration step replayed when the supervisor rebuilds a router.
pub(crate) type ReplayStep = Box<dyn Fn(&mut RestControlerService) + Send + Sync>;

/// Configuration pieces required to rebuild a [`RestControlerService`].
///
/// The supervisor keeps the snapshot of the original configuration and derives
/// the rebuilt snapshots from the dynamic settings (port, network mode,
/// entrypoint root).
#[derive(Clone)]
pub(crate) struct RestServiceSnapshot {
    /// IP address the server binds to (`None` means loopback).
    pub(crate) server_ip: Option<std::net::IpAddr>,
    /// Server port.
    pub(crate) server_port: u16,
    /// Networks whose clients are allowed to connect to the server.
    pub(crate) allowed_networks: Vec<ipnet::IpNet>,
    /// Public root path prefix, when set.
    pub(crate) entrypoint_root: Option<String>,
    /// API path segment mounted under the root prefix.
    pub(crate) entrypoint_api: Option<String>,
    /// Tray factory supplied by the application crate.
    pub(crate) tray_factory: Option<Arc<dyn ServerTrayFactory>>,
    /// Whether the server tray should be shown when a GUI is available.
    pub(crate) tray_enabled: bool,
    /// Callback reloading the application configuration from the tray.
    pub(crate) reload_configuration: Option<Arc<dyn Fn() -> String + Send + Sync>>,
}

impl RestServiceSnapshot {
    /// Returns the socket address the server binds to.
    pub(crate) fn socket_addr(&self) -> std::net::SocketAddr {
        std::net::SocketAddr::new(
            self.server_ip
                .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))),
            self.server_port,
        )
    }

    /// Returns the parsed entry point root path segments.
    ///
    /// Empty segments produced by leading, trailing or doubled slashes are
    /// dropped so every segment is safe to feed to `warp::path`.
    pub(crate) fn root_segments(&self) -> Vec<String> {
        self.entrypoint_root
            .as_deref()
            .filter(|root| !root.is_empty())
            .map(|root| {
                root.split('/')
                    .map(str::trim)
                    .filter(|segment| !segment.is_empty())
                    .map(ToString::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns the dynamic settings describing this snapshot.
    pub(crate) fn initial_settings(&self) -> RestServerSettings {
        let network_mode = if self.server_ip.map_or(true, |ip| ip.is_loopback()) {
            ServerNetworkMode::Local
        } else if self.allowed_networks.is_empty() {
            ServerNetworkMode::Public
        } else {
            ServerNetworkMode::Private
        };
        RestServerSettings {
            server_port: self.server_port,
            network_mode,
            entrypoint_root: self.entrypoint_root.clone(),
        }
    }
}

impl RestControlerService {
    /// Creates a new [`RestControlerService`] from the given configuration.
    ///
    /// # Arguments
    /// * `configuration` - REST controller configuration.
    ///
    /// # Returns
    /// A configured REST controller service.
    pub fn new(configuration: RestControlerConfiguration) -> Self {
        let socket_addr = if let Some(socket) = configuration.server_socket {
            socket
        } else {
            let ip = configuration
                .server_ip
                .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)));
            let port = configuration.server_port.unwrap_or(8080);
            std::net::SocketAddr::new(ip, port)
        };

        // Empty segments produced by leading, trailing or doubled slashes are
        // dropped so every segment is safe to feed to `warp::path`.
        let entrypoint_root = configuration
            .entrypoint_root
            .as_deref()
            .filter(|root| !root.is_empty())
            .map(|root| {
                root.split('/')
                    .map(str::trim)
                    .filter(|segment| !segment.is_empty())
                    .map(ToString::to_string)
                    .collect()
            })
            .unwrap_or_default();

        let entrypoint_api: String = configuration.entrypoint_api.unwrap_or("api".to_string());

        let (main_thread_dispatcher, main_thread_loop) = QueuedMainThreadDispatcher::new_pair();

        let snapshot = RestServiceSnapshot {
            server_ip: Some(socket_addr.ip()),
            server_port: socket_addr.port(),
            allowed_networks: configuration.allowed_networks.clone(),
            entrypoint_root: configuration.entrypoint_root.clone(),
            entrypoint_api: Some(entrypoint_api.clone()),
            tray_factory: configuration.tray_factory.clone(),
            tray_enabled: configuration.tray_enabled,
            reload_configuration: configuration.reload_configuration.clone(),
        };
        let supervisor = Arc::new(RestServerSupervisor::new(
            snapshot,
            Arc::clone(&main_thread_dispatcher),
        ));

        RestControlerService {
            socket_addr,
            router: None,
            allowed_networks: configuration.allowed_networks,
            entrypoint_root,
            entrypoint_api: Some(entrypoint_api),
            main_thread_dispatcher,
            main_thread_loop: Some(main_thread_loop),
            tray_factory: configuration.tray_factory,
            tray_enabled: configuration.tray_enabled,
            reload_configuration: configuration.reload_configuration,
            supervisor: Some(supervisor),
            recording: true,
            web_generation: new_web_generation(),
        }
    }

    /// Builds a rebuilt instance for the supervisor from the given snapshot.
    ///
    /// Rebuilt instances reuse the shared main-thread dispatcher and do not
    /// record replay steps (the supervisor replays the recorded ones).
    pub(crate) fn from_snapshot(
        snapshot: RestServiceSnapshot,
        dispatcher: Arc<QueuedMainThreadDispatcher>,
    ) -> Self {
        let entrypoint_root = snapshot.root_segments();
        RestControlerService {
            socket_addr: snapshot.socket_addr(),
            router: None,
            allowed_networks: snapshot.allowed_networks.clone(),
            entrypoint_root,
            entrypoint_api: snapshot.entrypoint_api.clone(),
            main_thread_dispatcher: dispatcher,
            main_thread_loop: None,
            tray_factory: snapshot.tray_factory.clone(),
            tray_enabled: snapshot.tray_enabled,
            reload_configuration: snapshot.reload_configuration.clone(),
            supervisor: None,
            recording: false,
            web_generation: web_instance_etag().to_string(),
        }
    }

    /// Assigns the server generation ETag (`T:`) for HTML documents.
    ///
    /// Called by the supervisor on every router (re)build before replaying
    /// registration steps, so rebuilt web asset filters capture the current
    /// generation instead of a stale one captured in a recorded step.
    ///
    /// # Arguments
    /// * `generation` - Fresh bare generation value from [`new_web_generation`].
    pub(crate) fn set_web_generation(&mut self, generation: String) {
        self.web_generation = generation;
    }

    /// Records a registration step so the supervisor can replay it.
    fn record_step(&mut self, step: ReplayStep) {
        if self.recording {
            if let Some(supervisor) = &self.supervisor {
                supervisor.record_step(step);
            }
        }
    }

    /// Finalizes a rebuilt router: adds the root redirect when the entrypoint
    /// root is configured and returns the combined router.
    pub(crate) fn finalize_router(&mut self) -> RestRouter {
        if !self.entrypoint_root.is_empty() {
            self.add_root_redirect();
        }
        self.router
            .clone()
            .expect("REST router replay produced no route.")
    }

    /// Returns the peer address of a request.
    ///
    /// The accept loop forwards the transport-level peer address through a
    /// controlled header (stripped and re-injected per connection, so it
    /// cannot be spoofed); `warp::addr::remote()` is used as the fallback.
    fn peer_addr_filter() -> BoxedFilter<(Option<std::net::SocketAddr>,)> {
        warp::header::optional::<String>(super::supervisor::PEER_ADDR_HEADER)
            .and(warp::addr::remote())
            .map(
                |injected: Option<String>, remote: Option<std::net::SocketAddr>| {
                    injected
                        .and_then(|value| value.parse::<std::net::SocketAddr>().ok())
                        .or(remote)
                },
            )
            .boxed()
    }

    /// Mounts a static directory under the configured root entry point.
    ///
    /// # Arguments
    /// * `directory_path` - Directory to expose as static files.
    /// * `path` - Relative path under the root entry point, or `""` for the root itself.
    #[allow(dead_code)]
    pub fn register_web_directory(&mut self, directory_path: &str, path: &str) {
        self.register_web_assets(WebAssetSource::directory(directory_path), path);
    }

    /// Mounts embedded frontend assets under the configured root entry point.
    ///
    /// # Arguments
    /// * `assets` - Embedded frontend asset provider supplied by the application crate.
    /// * `path` - Relative path under the root entry point, or `""` for the root itself.
    #[allow(dead_code)]
    pub fn register_embedded_web_assets(&mut self, assets: SharedWebAssets, path: &str) {
        self.register_web_assets(WebAssetSource::embedded(assets), path);
    }

    fn register_web_assets(&mut self, source: WebAssetSource, path: &str) {
        let mount_path = normalize_mount_path(path);
        let reserved_api = if mount_path.is_empty() {
            self.entrypoint_api.clone()
        } else {
            None
        };
        let entrypoint_root = self.entrypoint_root.clone();
        // HTML `<base>` used by this mount: the entrypoint root plus the mount
        // path, so a bundle mounted below the root rewrites its base marker to
        // its own mount point instead of the application root.
        let mut base_segments = entrypoint_root.clone();
        if !mount_path.is_empty() {
            base_segments.push(mount_path.clone());
        }
        let web_base = if base_segments.is_empty() {
            "/".to_string()
        } else {
            format!("/{}/", base_segments.join("/"))
        };
        // Embedded bundles mounted below the root live inside the shared asset
        // pool under the mount path (e.g. `admin/`); scope the provider so
        // asset lookup and the SPA fallback stay inside the bundle instead of
        // falling back to the root bundle document.
        let source = scope_web_asset_source(source, &mount_path);
        // Conditional validation only applies to embedded bundles: directory
        // sources are mutable on disk, so they never carry an ETag.
        let conditional = source.is_embedded();
        let source = Arc::new(source);
        // HTML ETag bound to this router build: regenerated on every server
        // (re)start because `replace_html_base` output depends on the runtime
        // root. Captured here (build time), never in the recorded replay step.
        let generation = self.web_generation.clone();
        let instance = web_instance_etag().to_string();
        let new_filter = self
            .make_base_filter(false, &mount_path)
            .and(warp::get())
            .and(warp::path::tail())
            .and(warp::header::optional::<String>("if-none-match"))
            .and_then(move |tail: warp::path::Tail, if_none_match: Option<String>| {
                let reserved_api = reserved_api.clone();
                let web_base = web_base.clone();
                let source = Arc::clone(&source);
                let generation = generation.clone();
                let instance = instance.clone();
                async move {
                    let request_path = tail.as_str();
                    if let Some(api_entrypoint) = &reserved_api {
                        let first_segment = request_path.split('/').next().unwrap_or_default();
                        if first_segment == api_entrypoint {
                            return Err(warp::reject::not_found());
                        }
                    }

                    match source.load(request_path) {
                        Ok(asset) => {
                            if conditional {
                                let (etag_header, bare_etag) =
                                    asset_etag_for(asset.mime_type, &generation, &instance);
                                if etag_not_modified(if_none_match.as_deref(), &bare_etag) {
                                    let response = warp::http::Response::builder()
                                        .status(StatusCode::NOT_MODIFIED)
                                        .header("etag", etag_header)
                                        .body(warp::hyper::Body::empty())
                                        .expect("failed to build 304 web asset response");
                                    return Ok::<RestReply, Rejection>((
                                        Box::new(response) as Box<dyn Reply + Send>,
                                    ));
                                }
                                let bytes = if asset.mime_type.starts_with("text/html") {
                                    replace_html_base(asset.bytes, &web_base)
                                } else {
                                    asset.bytes
                                };
                                let response = reply::with_header(bytes, "content-type", asset.mime_type);
                                let response =
                                    reply::with_header(response, "etag", etag_header);
                                return Ok::<RestReply, Rejection>((
                                    Box::new(response) as Box<dyn Reply + Send>,
                                ));
                            }
                            let bytes = if asset.mime_type.starts_with("text/html") {
                                replace_html_base(asset.bytes, &web_base)
                            } else {
                                asset.bytes
                            };
                            let response =
                                reply::with_header(bytes, "content-type", asset.mime_type);
                            Ok::<RestReply, Rejection>((
                                Box::new(response) as Box<dyn Reply + Send>,
                            ))
                        }
                        Err(_) => Err(warp::reject::not_found()),
                    }
                }
            })
            .boxed();

        self.add_route(new_filter);
    }

    /// Builds the client access-control filter.
    ///
    /// When networks are configured, the filter rejects requests whose remote
    /// address does not belong to one of them; otherwise it lets everything
    /// through. This powers `ServerNetworkMode::Private`.
    fn make_client_acl_filter(&self) -> BoxedFilter<()> {
        let networks = self.allowed_networks.clone();
        if networks.is_empty() {
            warp::any().boxed()
        } else {
            Self::peer_addr_filter()
                .and_then(move |addr: Option<std::net::SocketAddr>| {
                    let networks = networks.clone();
                    async move {
                        match addr {
                            Some(addr) if networks.iter().any(|net| net.contains(&addr.ip())) => {
                                Ok(())
                            }
                            _ => Err(warp::reject::not_found()),
                        }
                    }
                })
                .untuple_one()
                .boxed()
        }
    }

    fn add_route(&mut self, route: RestRouter) {
        let route = self.make_client_acl_filter().and(route).boxed();
        if self.router.is_none() {
            self.router = Some(route);
        } else {
            self.router = Some(self.router.clone().unwrap().or(route).unify().boxed());
        }
    }

    /// Registers a `302 Found` redirect from the server root `/` to the
    /// configured entry point root path.
    ///
    /// When the web application is mounted under a path prefix (non-empty
    /// `--entrypoint-root`), the bare server root otherwise matches nothing and
    /// returns `404 Not Found`; the redirect keeps `http://host:port/`
    /// reachable. The `Location` header is built as a relative path prefixed
    /// with `/` so it resolves against the server origin.
    fn add_root_redirect(&mut self) {
        let entrypoint_root = self.entrypoint_root.clone();
        let redirect_filter = warp::path::end()
            .map(move || {
                let target = format!("/{}/", entrypoint_root.join("/"));
                let response = warp::http::Response::builder()
                    .status(StatusCode::FOUND)
                    .header("location", target)
                    .body(Vec::new())
                    .expect("Failed to build root redirect response.");
                (Box::new(response) as Box<dyn Reply + Send>,)
            })
            .boxed();

        self.add_route(redirect_filter);
    }

    fn make_base_filter(&self, use_api: bool, last_path: &str) -> BoxedFilter<()> {
        let mut path = self.entrypoint_root.clone();

        if use_api && self.entrypoint_api.is_some() {
            path.push(self.entrypoint_api.as_ref().unwrap().clone());
        }

        if !last_path.is_empty() {
            path.extend(
                last_path
                    .split('/')
                    .map(str::trim)
                    .filter(|segment| !segment.is_empty())
                    .map(ToString::to_string),
            );
        }

        let mut base_filter;
        if path.is_empty() {
            return warp::any().boxed();
        } else {
            base_filter = warp::path(path[0].clone()).boxed();
            for segment in path.iter().skip(1) {
                base_filter = base_filter.and(warp::path(segment.clone())).boxed();
            }
        }

        base_filter
    }

    async fn call_and_reply(
        call: &SerializedControlerFunction,
        input: ControlerFunctionInput,
    ) -> (Box<dyn Reply + Send>,) {
        let response: Box<dyn Reply + Send> = match call(input).await {
            Ok(response) => Box::new(reply::json(&response)),
            Err(error) => Box::new(reply::with_status(
                reply::json(&error),
                StatusCode::BAD_REQUEST,
            )),
        };

        (response,)
    }

    /// Executes a header-aware JSON command and builds the HTTP reply.
    ///
    /// Honors the command-provided status code (e.g. `304 Not Modified`) and
    /// additional response headers (e.g. `ETag`). A `304` reply carries no body.
    async fn call_and_reply_json(
        call: &JsonControlerFunction,
        input: ControlerJsonInput,
    ) -> (Box<dyn Reply + Send>,) {
        let response: Box<dyn Reply + Send> = match call(input).await {
            Ok(ControlerJsonOutput {
                value,
                status,
                headers,
            }) => {
                let mut builder = warp::http::Response::builder()
                    .status(StatusCode::from_u16(status).unwrap_or(StatusCode::OK))
                    .header("content-type", "application/json");
                for (name, header_value) in &headers {
                    if name.to_ascii_lowercase() != "content-type" {
                        builder = builder.header(name.as_str(), header_value.as_str());
                    }
                }
                let body = if status == 304 {
                    Vec::new()
                } else {
                    serde_json::to_vec(&value).unwrap_or_else(|_| Vec::new())
                };
                Box::new(builder.body(body).expect("Failed to build JSON response."))
            }
            Err(error) => Box::new(reply::with_status(
                reply::json(&error),
                StatusCode::BAD_REQUEST,
            )),
        };

        (response,)
    }

    async fn call_and_reply_stream(
        call: &StreamControlerFunction,
        input: ControlerStreamInput,
    ) -> RestReply {
        let is_head = input.method.eq_ignore_ascii_case("HEAD");
        match call(input).await {
            Ok(ControlerStreamOutput {
                status,
                body,
                content_type,
                headers,
            }) => {
                let body = match body {
                    ResponseBody::Buffered(mut bytes) => {
                        if is_head {
                            bytes.clear();
                        }
                        warp::hyper::Body::from(bytes)
                    }
                    ResponseBody::Streamed(stream) => {
                        if is_head {
                            warp::hyper::Body::empty()
                        } else {
                            warp::hyper::Body::wrap_stream(stream)
                        }
                    }
                };
                let mut builder = warp::http::Response::builder()
                    .status(StatusCode::from_u16(status).unwrap_or(StatusCode::OK));
                builder = builder.header("content-type", &content_type);

                for (name, value) in &headers {
                    if name.to_ascii_lowercase() != "content-type" {
                        builder = builder.header(name.as_str(), value.as_str());
                    }
                }

                let response = builder
                    .body(body)
                    .expect("Failed to build stream response.");
                (Box::new(response) as Box<dyn Reply + Send>,)
            }
            Err(error) => {
                let response = warp::http::Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header("content-type", "text/plain; charset=utf-8")
                    .body(error.into_bytes())
                    .expect("Failed to build stream error response.");
                (Box::new(response) as Box<dyn Reply + Send>,)
            }
        }
    }

    fn headers_to_map(
        headers: &warp::http::HeaderMap,
    ) -> std::collections::HashMap<String, String> {
        let mut map = std::collections::HashMap::new();
        for (name, value) in headers.iter() {
            if let Ok(v) = value.to_str() {
                map.insert(name.to_string(), v.to_string());
            }
        }
        map
    }

    /// Returns the peer address of a request, falling back to the unspecified
    /// address when the transport does not expose one.
    ///
    /// The unspecified address is never loopback, so transports without a peer
    /// address are always treated as remote (untrusted) clients.
    fn remote_peer(addr: Option<std::net::SocketAddr>) -> std::net::SocketAddr {
        addr.unwrap_or_else(|| {
            std::net::SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED), 0)
        })
    }
}

impl Default for RestControlerService {
    fn default() -> Self {
        RestControlerService::new(RestControlerConfiguration::default())
    }
}

impl ControlerService for RestControlerService {
    fn register_serialized_function(&mut self, command: &str, call: SerializedControlerFunction) {
        let replay_command = command.to_string();
        let replay_call = Arc::clone(&call);
        self.record_step(Box::new(move |svc| {
            ControlerService::register_serialized_function(
                svc,
                &replay_command,
                Arc::clone(&replay_call),
            );
        }));

        let base_filter = self.make_base_filter(true, command);

        let post_call = call.clone();
        let post_filter = base_filter
            .clone()
            .and(warp::post())
            .and(warp::body::json::<Value>())
            .and_then(move |input: Value| {
                let post_call = Arc::clone(&post_call);
                async move {
                    Ok::<(Box<dyn Reply + Send>,), Rejection>(
                        Self::call_and_reply(&post_call, ControlerFunctionInput::Json(input)).await,
                    )
                }
            });

        let get_call = call.clone();
        let get_filter = base_filter
            .clone()
            .and(warp::get())
            .and(warp::query::raw().or(warp::any().map(String::new)).unify())
            .and_then(move |input: String| {
                let get_call = Arc::clone(&get_call);
                async move {
                    Ok::<(Box<dyn Reply + Send>,), Rejection>(
                        Self::call_and_reply(&get_call, ControlerFunctionInput::Query(input)).await,
                    )
                }
            });

        self.add_route(post_filter.or(get_filter).unify().boxed());
    }

    fn register_json_function(&mut self, command: &str, call: JsonControlerFunction) {
        let replay_command = command.to_string();
        let replay_call = Arc::clone(&call);
        self.record_step(Box::new(move |svc| {
            ControlerService::register_json_function(
                svc,
                &replay_command,
                Arc::clone(&replay_call),
            );
        }));

        let base_filter = self.make_base_filter(true, command);

        let post_call = call.clone();
        let post_filter = base_filter
            .clone()
            .and(warp::post())
            .and(Self::peer_addr_filter())
            .and(warp::body::json::<Value>())
            .and(warp::header::headers_cloned())
            .and_then(
                move |addr: Option<std::net::SocketAddr>,
                      input: Value,
                      headers: warp::http::HeaderMap| {
                    let post_call = Arc::clone(&post_call);
                    async move {
                        Ok::<(Box<dyn Reply + Send>,), Rejection>(
                            Self::call_and_reply_json(
                                &post_call,
                                ControlerJsonInput {
                                    payload: ControlerFunctionInput::Json(input),
                                    context: RequestControlerContext::new(Self::headers_to_map(
                                        &headers,
                                    ))
                                    .with_remote_addr(Self::remote_peer(addr))
                                    .with_method("POST"),
                                },
                            )
                            .await,
                        )
                    }
                },
            );

        let get_call = call.clone();
        let get_filter = base_filter
            .clone()
            .and(warp::get())
            .and(Self::peer_addr_filter())
            .and(warp::query::raw().or(warp::any().map(String::new)).unify())
            .and(warp::header::headers_cloned())
            .and_then(
                move |addr: Option<std::net::SocketAddr>,
                      input: String,
                      headers: warp::http::HeaderMap| {
                    let get_call = Arc::clone(&get_call);
                    async move {
                        Ok::<(Box<dyn Reply + Send>,), Rejection>(
                            Self::call_and_reply_json(
                                &get_call,
                                ControlerJsonInput {
                                    payload: ControlerFunctionInput::Query(input),
                                    context: RequestControlerContext::new(Self::headers_to_map(
                                        &headers,
                                    ))
                                    .with_remote_addr(Self::remote_peer(addr))
                                    .with_method("GET"),
                                },
                            )
                            .await,
                        )
                    }
                },
            );

        self.add_route(post_filter.or(get_filter).unify().boxed());
    }

    fn stream_public_path(&self, command: &str) -> String {
        let mut parts: Vec<String> = self
            .entrypoint_root
            .iter()
            .flat_map(|segment| segment.split('/'))
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .map(ToString::to_string)
            .collect();

        if let Some(api) = &self.entrypoint_api {
            parts.extend(
                api.split('/')
                    .map(str::trim)
                    .filter(|segment| !segment.is_empty())
                    .map(ToString::to_string),
            );
        }

        parts.extend(
            command
                .split('/')
                .map(str::trim)
                .filter(|segment| !segment.is_empty())
                .map(ToString::to_string),
        );

        if parts.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", parts.join("/"))
        }
    }

    fn register_stream_function(&mut self, command: &str, call: StreamControlerFunction) {
        let replay_command = command.to_string();
        let replay_call = Arc::clone(&call);
        self.record_step(Box::new(move |svc| {
            ControlerService::register_stream_function(
                svc,
                &replay_command,
                Arc::clone(&replay_call),
            );
        }));

        let base_filter = self.make_base_filter(true, command);

        // Build the entry-point URL prefix for this command
        let entry_point = {
            let base = if self.socket_addr.is_ipv6() {
                format!(
                    "http://[{}]:{}",
                    self.socket_addr.ip(),
                    self.socket_addr.port()
                )
            } else {
                format!(
                    "http://{}:{}",
                    self.socket_addr.ip(),
                    self.socket_addr.port()
                )
            };
            if self.entrypoint_root.is_empty() && self.entrypoint_api.is_none() {
                format!("{}/{}", base, command)
            } else {
                let mut parts = self.entrypoint_root.clone();
                if let Some(ref api) = self.entrypoint_api {
                    parts.push(api.clone());
                }
                parts.push(command.to_string());
                format!("{}/{}", base, parts.join("/"))
            }
        };

        // Build a unified filter that captures: tail, method, headers, body
        // Body is optional - empty body for GET/HEAD/OPTIONS/DELETE
        let query_filter = warp::query::raw()
            .or(warp::any().map(String::new))
            .unify()
            .boxed();

        // Body extraction: try bytes first, fall back to empty
        let body_filter = warp::body::bytes()
            .map(|b: warp::hyper::body::Bytes| b.to_vec())
            .or(warp::any().map(Vec::new))
            .unify()
            .boxed();

        let stream_filter = base_filter
            .and(warp::path::tail())
            .and(warp::method())
            .and(warp::header::headers_cloned())
            .and(query_filter)
            .and(body_filter)
            .and_then(
                move |tail: warp::path::Tail,
                      method: warp::http::Method,
                      headers: warp::http::HeaderMap,
                      query: String,
                      body: Vec<u8>| {
                    let call = Arc::clone(&call);
                    let entry_point = entry_point.clone();
                    async move {
                        let input = ControlerStreamInput {
                            path: tail.as_str().to_string(),
                            query,
                            method: method.to_string(),
                            headers: Self::headers_to_map(&headers),
                            body,
                            entry_point,
                            force_buffer_response: false,
                        };
                        Ok::<RestReply, Rejection>(Self::call_and_reply_stream(&call, input).await)
                    }
                },
            )
            .boxed();

        self.add_route(stream_filter);
    }

    fn register_web_directory(&mut self, directory_path: &str, path: &str) {
        let replay_directory = directory_path.to_string();
        let replay_path = path.to_string();
        self.record_step(Box::new(move |svc| {
            ControlerService::register_web_directory(svc, &replay_directory, &replay_path);
        }));
        RestControlerService::register_web_directory(self, directory_path, path);
    }

    fn register_embedded_web_assets(&mut self, assets: SharedWebAssets, path: &str) {
        let replay_path = path.to_string();
        let replay_assets = Arc::clone(&assets);
        self.record_step(Box::new(move |svc| {
            ControlerService::register_embedded_web_assets(
                svc,
                Arc::clone(&replay_assets),
                &replay_path,
            );
        }));
        RestControlerService::register_embedded_web_assets(self, assets, path);
    }

    fn main_thread_dispatcher(&self) -> Option<Arc<dyn MainThreadDispatcher>> {
        let dispatcher: Arc<dyn MainThreadDispatcher> = self.main_thread_dispatcher.clone();
        Some(dispatcher)
    }

    fn rest_server_handle(&self) -> Option<Arc<RestServerHandle>> {
        self.supervisor
            .as_ref()
            .map(|supervisor| Arc::new(RestServerHandle::for_supervisor(Arc::clone(supervisor))))
    }

    fn launch(&mut self) {
        let supervisor = self
            .supervisor
            .clone()
            .expect("REST controller supervisor is missing.");

        // Record the root redirect so router replays include it, with the
        // root active at replay time.
        supervisor.record_step(Box::new(|svc: &mut RestControlerService| {
            if !svc.entrypoint_root.is_empty() {
                svc.add_root_redirect();
            }
        }));

        // When the web application is served under a path prefix, redirect the
        // bare server root to the configured entry point. The redirect is added
        // last so every registered route keeps priority.
        if !self.entrypoint_root.is_empty() {
            self.add_root_redirect();
        }

        let router = self
            .router
            .clone()
            .expect("REST controller launched without any registered route.");
        let socket_addr = self.socket_addr;
        let entrypoint_root = self.entrypoint_root.clone();
        let main_thread_loop = self
            .main_thread_loop
            .take()
            .expect("REST controller launched more than once.");
        let dispatcher: Arc<dyn MainThreadDispatcher> = self.main_thread_dispatcher.clone();
        install_global_main_thread_dispatcher(dispatcher)
            .expect("Failed to install the REST main-thread dispatcher.");

        // The supervisor owns the listener and the accept loop from now on.
        let shutdown = supervisor.launch_controller(router, socket_addr, &entrypoint_root);

        // Show a tray icon in server mode only, and only when a graphical
        // environment is available (e.g. not a headless Linux server) and an
        // application-supplied tray factory is present. The tray receives the
        // shared shutdown signal so its "close" action can stop the HTTP server.
        let tray = if self.tray_enabled && gui_available() {
            self.tray_factory.as_ref().map(|factory| {
                let application_root = if self.entrypoint_root.is_empty() {
                    "/".to_string()
                } else {
                    format!("/{}", self.entrypoint_root.join("/"))
                };
                let configuration = ServerTrayConfiguration {
                    server_url: build_public_url(self.socket_addr, &self.entrypoint_root),
                    admin_url: {
                        let mut admin_url =
                            build_public_url(self.socket_addr, &self.entrypoint_root);
                        if !admin_url.ends_with('/') {
                            admin_url.push('/');
                        }
                        admin_url.push_str("admin/");
                        admin_url
                    },
                    server_display_url: format!(
                        "http://{}:{}{}",
                        display_server_host(self.socket_addr),
                        self.socket_addr.port(),
                        application_root
                    ),
                    network_mode: if self.socket_addr.ip().is_loopback() {
                        "Local".to_string()
                    } else if self.allowed_networks.is_empty() {
                        "Public".to_string()
                    } else {
                        "Private".to_string()
                    },
                    log_cache: crate::logger::global_log_cache(),
                    shutdown: shutdown.clone(),
                    reload_configuration: self.reload_configuration.clone(),
                    handle_sink: Some(supervisor.tray_handle_sink()),
                };
                (Arc::clone(factory), configuration)
            })
        } else {
            None
        };

        // On macOS the Tauri event loop must run on the process main thread, so
        // this (main) thread hosts the tray while the HTTP server and the
        // logical main-thread loop run on their own threads. The tray blocks
        // this thread until the user closes the server; its "shutdown" action
        // then unblocks the server and the logical main-thread loop.
        #[cfg(target_os = "macos")]
        {
            let loop_shutdown = shutdown.clone();
            let main_thread_thread = std::thread::spawn(move || {
                main_thread_loop.run_until(|| loop_shutdown.is_requested());
            });

            if let Some((factory, configuration)) = tray {
                let _handle = factory.run_on_main_thread(configuration);
            } else {
                // No tray: keep the main thread alive (the logical main-thread
                // loop runs on its own thread) until the server shuts down.
                wait_for_shutdown(&shutdown);
            }

            let _ = main_thread_thread.join();
            supervisor.join_server();
            return;
        }

        #[cfg(not(target_os = "macos"))]
        if let Some((factory, configuration)) = tray {
            let _handle = factory.spawn_tray(configuration);
        }

        // Block until the server is requested to shut down, then wait for the
        // HTTP server to finish its graceful shutdown before returning.
        #[cfg(not(target_os = "macos"))]
        {
            main_thread_loop.run_until(|| shutdown.is_requested());
            supervisor.join_server();
        }
    }
}

/// Blocks the calling thread until shutdown is requested.
///
/// Used on macOS when no server tray is present: the logical main-thread loop
/// runs on its own thread, and this keeps the process main thread alive until
/// the server shuts down (mirroring `MainThreadDispatchLoop::run_until`'s
/// blocking behaviour on the other platforms).
#[cfg(target_os = "macos")]
fn wait_for_shutdown(shutdown: &ShutdownSignal) {
    const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);
    while !shutdown.is_requested() {
        std::thread::sleep(POLL_INTERVAL);
    }
}
