//! REST controller backend built on top of Warp.

use std::sync::Arc;

use serde_json::Value;
use warp::filters::BoxedFilter;
use warp::http::StatusCode;
use warp::{reply, Filter, Rejection, Reply};

use super::web_assets::{normalize_mount_path, WebAssetSource};
use super::{
    install_global_main_thread_dispatcher, main_thread::MainThreadDispatchLoop,
    main_thread::QueuedMainThreadDispatcher, ControlerFunctionInput, ControlerService,
    ControlerStreamInput, ControlerStreamOutput, MainThreadDispatcher, SerializedControlerFunction,
    SharedWebAssets, StreamControlerFunction,
};

type RestReply = (Box<dyn Reply + Send>,);
type RestRouter = BoxedFilter<(RestReply,)>;

/// Configuration options for the REST controller backend.
#[derive(Default)]
pub struct RestControlerConfiguration {
    /// Server socket.
    server_socket: Option<std::net::SocketAddr>,

    /// Server port.
    server_port: Option<u16>,

    /// Server IP.
    server_ip: Option<std::net::IpAddr>,

    /// Entry point root or None
    entrypoint_root: Option<String>,

    /// Entry point for API
    entrypoint_api: Option<String>,
}

impl RestControlerConfiguration {
    /// Sets the socket address used by the HTTP server.
    ///
    /// # Arguments
    /// * `server_socket` - Socket address to bind to.
    ///
    /// # Returns
    /// The updated configuration.
    #[allow(dead_code)]
    pub fn server_socket(mut self, server_socket: std::net::SocketAddr) -> Self {
        self.server_socket = Some(server_socket);
        self
    }

    /// Sets the IP address used by the HTTP server.
    ///
    /// # Arguments
    /// * `server_ip` - Server IP address.
    ///
    /// # Returns
    /// The updated configuration.
    #[allow(dead_code)]
    pub fn server_ip(mut self, server_ip: std::net::IpAddr) -> Self {
        self.server_ip = Some(server_ip);
        self
    }

    /// Sets the port used by the HTTP server.
    ///
    /// # Arguments
    /// * `server_port` - Server port.
    ///
    /// # Returns
    /// The updated configuration.
    #[allow(dead_code)]
    pub fn server_port(mut self, server_port: u16) -> Self {
        self.server_port = Some(server_port);
        self
    }

    /// Sets the root path prefix mounted by the HTTP server.
    ///
    /// # Arguments
    /// * `entrypoint_root` - Root path prefix.
    ///
    /// # Returns
    /// The updated configuration.
    #[allow(dead_code)]
    pub fn entrypoint_root(mut self, entrypoint_root: &str) -> Self {
        self.entrypoint_root = Some(entrypoint_root.to_string());
        self
    }

    /// Sets the API path segment mounted under the root prefix.
    ///
    /// # Arguments
    /// * `entrypoint_api` - API path segment.
    ///
    /// # Returns
    /// The updated configuration.
    #[allow(dead_code)]
    pub fn entrypoint_api(mut self, entrypoint_api: &str) -> Self {
        self.entrypoint_api = Some(entrypoint_api.to_string());
        self
    }
}

/// REST controller service.
///
/// This backend exposes registered commands over HTTP using JSON payloads.
pub struct RestControlerService {
    socket_addr: std::net::SocketAddr,
    router: Option<RestRouter>,
    entrypoint_root: Vec<String>,
    entrypoint_api: Option<String>,
    main_thread_dispatcher: Arc<QueuedMainThreadDispatcher>,
    main_thread_loop: Option<MainThreadDispatchLoop>,
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
            entrypoint_root,
            entrypoint_api: Some(entrypoint_api),
            main_thread_dispatcher,
            main_thread_loop: Some(main_thread_loop),
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
        let source = Arc::new(source);
        let new_filter = self
            .make_base_filter(false, &mount_path)
            .and(warp::get())
            .and(warp::path::tail())
            .and_then(move |tail: warp::path::Tail| {
                let reserved_api = reserved_api.clone();
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
                            let response =
                                reply::with_header(asset.bytes, "content-type", asset.mime_type);
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

    fn add_route(&mut self, route: RestRouter) {
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

    async fn call_and_reply_stream(
        call: &StreamControlerFunction,
        input: ControlerStreamInput,
    ) -> RestReply {
        let is_head = input.method.eq_ignore_ascii_case("HEAD");
        match call(input).await {
            Ok(ControlerStreamOutput {
                status,
                mut body,
                content_type,
                headers,
            }) => {
                if is_head {
                    body.clear();
                }
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
    /// # Arguments
    ///
    /// * `router` - Fully configured Warp router.
    /// * `socket_addr` - Socket address to bind to.
    /// * `entrypoint_root` - Root path segments used for display purposes.
    fn run_server(
        router: RestRouter,
        socket_addr: std::net::SocketAddr,
        entrypoint_root: Vec<String>,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create the REST runtime.");
        rt.block_on(async move {
            let serv = warp::serve(router).run(socket_addr);
            let mut enpoint: String;
            if entrypoint_root.is_empty() {
                enpoint = "".to_string()
            } else {
                enpoint = entrypoint_root.join("/");
                enpoint.push('/')
            }
            println!(
                "Web Server Application launched: http://{}:{}/{}",
                socket_addr.ip(),
                socket_addr.port(),
                enpoint
            );
            serv.await;
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
            .and(warp::query::raw())
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

        std::thread::spawn(move || {
            Self::run_server(router, socket_addr, entrypoint_root);
        });
        main_thread_loop.run();
    }
}
