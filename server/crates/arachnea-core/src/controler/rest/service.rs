//! REST controller service based on Warp.

use std::sync::Arc;

use serde_json::Value;
use warp::filters::BoxedFilter;
use warp::http::StatusCode;
use warp::{reply, Filter, Rejection, Reply};

use crate::controler::web_assets::{normalize_mount_path, replace_html_base, WebAssetSource};
use crate::controler::{
    install_global_main_thread_dispatcher, main_thread::MainThreadDispatchLoop,
    main_thread::QueuedMainThreadDispatcher, ControlerFunctionInput, ControlerJsonInput,
    ControlerJsonOutput, ControlerService, ControlerStreamInput, ControlerStreamOutput,
    JsonControlerFunction, MainThreadDispatcher, ResponseBody, SerializedControlerFunction,
    SharedWebAssets, StreamControlerFunction,
};

use super::configuration::RestControlerConfiguration;
use super::shutdown::ShutdownSignal;
use super::tray::{gui_available, ServerTrayConfiguration, ServerTrayFactory};
use super::{build_public_url, RestReply, RestRouter};

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

        let entrypoint_root = configuration
            .entrypoint_root
            .as_deref()
            .filter(|root| !root.is_empty())
            .map(|root| root.split('/').map(ToString::to_string).collect())
            .unwrap_or_default();

        let entrypoint_api: String = configuration.entrypoint_api.unwrap_or("api".to_string());

        let (main_thread_dispatcher, main_thread_loop) = QueuedMainThreadDispatcher::new_pair();

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
        }
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
        let web_base = if entrypoint_root.is_empty() {
            "/".to_string()
        } else {
            format!("/{}/", entrypoint_root.join("/"))
        };
        let source = Arc::new(source);
        let new_filter = self
            .make_base_filter(false, &mount_path)
            .and(warp::get())
            .and(warp::path::tail())
            .and_then(move |tail: warp::path::Tail| {
                let reserved_api = reserved_api.clone();
                let web_base = web_base.clone();
                let source = Arc::clone(&source);
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
            warp::addr::remote()
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

    fn make_base_filter(&self, use_api: bool, last_path: &str) -> BoxedFilter<()> {
        let mut path = self.entrypoint_root.clone();

        if use_api && self.entrypoint_api.is_some() {
            path.push(self.entrypoint_api.as_ref().unwrap().clone());
        }

        if !last_path.is_empty() {
            path.push(last_path.to_string());
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
                Box::new(
                    builder
                        .body(body)
                        .expect("Failed to build JSON response."),
                )
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

    /// Runs the Warp server to completion on a dedicated Tokio runtime.
    ///
    /// The server shuts down gracefully once the provided shutdown signal is
    /// requested: it stops accepting new connections and drains in-flight ones.
    ///
    /// # Arguments
    ///
    /// * `router` - Fully configured Warp router.
    /// * `socket_addr` - Socket address to bind to.
    /// * `entrypoint_root` - Root path segments used for display purposes.
    /// * `shutdown` - Shared signal that starts the graceful shutdown.
    fn run_server(
        router: RestRouter,
        socket_addr: std::net::SocketAddr,
        entrypoint_root: Vec<String>,
        shutdown: ShutdownSignal,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create the REST runtime.");
        rt.block_on(async move {
            let server_shutdown = shutdown.clone();
            let (addr, server) =
                warp::serve(router).bind_with_graceful_shutdown(socket_addr, async move {
                    server_shutdown.wait().await;
                });
            let mut enpoint: String;
            if entrypoint_root.is_empty() {
                enpoint = "".to_string()
            } else {
                enpoint = entrypoint_root.join("/");
                enpoint.push('/')
            }
            let display_host = if addr.ip().is_loopback() {
                "localhost".to_string()
            } else if addr.ip().is_unspecified() {
                if_addrs::get_if_addrs()
                    .ok()
                    .and_then(|interfaces| {
                        interfaces
                            .iter()
                            .find(|iface| {
                                !iface.is_loopback()
                                    && matches!(iface.addr, if_addrs::IfAddr::V4(_))
                            })
                            .map(|iface| iface.ip().to_string())
                    })
                    .unwrap_or_else(|| "localhost".to_string())
            } else {
                addr.ip().to_string()
            };
            println!(
                "Web Server Application launched: http://{}:{}/{}",
                display_host,
                addr.port(),
                enpoint
            );
            server.await;
            tracing::info!("HTTP server stopped");
        });
    }
}

impl Default for RestControlerService {
    fn default() -> Self {
        RestControlerService::new(RestControlerConfiguration::default())
    }
}

impl ControlerService for RestControlerService {
    fn register_serialized_function(&mut self, command: &str, call: SerializedControlerFunction) {
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
        let base_filter = self.make_base_filter(true, command);

        let post_call = call.clone();
        let post_filter = base_filter
            .clone()
            .and(warp::post())
            .and(warp::body::json::<Value>())
            .and(warp::header::headers_cloned())
            .and_then(move |input: Value, headers: warp::http::HeaderMap| {
                let post_call = Arc::clone(&post_call);
                async move {
                    Ok::<(Box<dyn Reply + Send>,), Rejection>(
                        Self::call_and_reply_json(
                            &post_call,
                            ControlerJsonInput {
                                payload: ControlerFunctionInput::Json(input),
                                headers: Self::headers_to_map(&headers),
                            },
                        )
                        .await,
                    )
                }
            });

        let get_call = call.clone();
        let get_filter = base_filter
            .clone()
            .and(warp::get())
            .and(warp::query::raw().or(warp::any().map(String::new)).unify())
            .and(warp::header::headers_cloned())
            .and_then(move |input: String, headers: warp::http::HeaderMap| {
                let get_call = Arc::clone(&get_call);
                async move {
                    Ok::<(Box<dyn Reply + Send>,), Rejection>(
                        Self::call_and_reply_json(
                            &get_call,
                            ControlerJsonInput {
                                payload: ControlerFunctionInput::Query(input),
                                headers: Self::headers_to_map(&headers),
                            },
                        )
                        .await,
                    )
                }
            });

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
        RestControlerService::register_web_directory(self, directory_path, path);
    }

    fn register_embedded_web_assets(&mut self, assets: SharedWebAssets, path: &str) {
        RestControlerService::register_embedded_web_assets(self, assets, path);
    }

    fn main_thread_dispatcher(&self) -> Option<Arc<dyn MainThreadDispatcher>> {
        let dispatcher: Arc<dyn MainThreadDispatcher> = self.main_thread_dispatcher.clone();
        Some(dispatcher)
    }

    fn launch(&mut self) {
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

        let shutdown = ShutdownSignal::new();

        let server_shutdown = shutdown.clone();
        let server_thread = std::thread::spawn(move || {
            Self::run_server(router, socket_addr, entrypoint_root, server_shutdown);
        });

        // Show a tray icon in server mode only, and only when a graphical
        // environment is available (e.g. not a headless Linux server) and an
        // application-supplied tray factory is present. The tray receives the
        // shared shutdown signal so its "close" action can stop the HTTP server.
        let tray = if self.tray_enabled && gui_available() {
            self.tray_factory.as_ref().map(|factory| {
                let configuration = ServerTrayConfiguration {
                    server_url: build_public_url(self.socket_addr, &self.entrypoint_root),
                    log_cache: crate::logger::global_log_cache(),
                    shutdown: shutdown.clone(),
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
            let _ = server_thread.join();
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
            let _ = server_thread.join();
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

