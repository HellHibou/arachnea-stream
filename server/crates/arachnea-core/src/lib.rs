#![warn(missing_docs)]
//! Shared Arachnea backend primitives.

/// Default maximum number of HTTP redirects followed by Arachnea HTTP clients.
pub const DEFAULT_MAX_REDIRECTS: usize = 16;

/// Controller abstractions and REST/Tauri backends.
pub mod controler;

/// Persistence contracts and runtime resource helpers.
pub mod persistence;

/// Logging initialization helpers shared by backend crates.
pub mod logger;

/// Thread-safe error correlation code generator.
pub mod error_code;

/// Cryptographic and hashing helpers (base62 encoding, ...).
pub mod crypt;

/// Various application resources (application path, ...)
pub mod application;

// Crate-owned constants retain their historical import paths through reexports.

// --- Application defaults ---

/// Default TCP port used by REST controllers and application options.
pub const DEFAULT_SERVER_PORT: u16 = 8080;

/// Default web scheme in generic application options; distinct from the Tauri controller fallback.
pub const DEFAULT_TAURI_WEB_SCHEME: &str = "arachnea-core";

/// Default API segment in application options, without surrounding slashes.
pub const DEFAULT_TAURI_API_PREFIX: &str = "api";

/// Whether generic application options enable a tray when a GUI is available.
pub(crate) const APPLICATION_DEFAULT_TRAY_ENABLED: bool = true;

// --- HTTP and web contracts ---

/// HTTP request header carrying conditional entity validators.
pub const HEADER_IF_NONE_MATCH: &str = "if-none-match";

/// HTTP response header carrying an entity validator.
pub const HEADER_ETAG: &str = "etag";

/// Placeholder replaced with the runtime mount base in served HTML.
pub(crate) const HTML_BASE_PLACEHOLDER: &str = "{base}";

// --- Persistence contracts ---

/// Default credentials file path relative to the application data directory.
pub const DEFAULT_FILE_CREDENTIALS_STORE_PATH: &str = "data/credentials.json";

/// Version of the plain credentials document format; a persistence compatibility invariant.
pub(crate) const CREDENTIALS_DOCUMENT_VERSION: u32 = 1;

/// Version of the AES-GCM credentials envelope; a persistence compatibility invariant.
pub(crate) const ENCRYPTED_CREDENTIALS_DOCUMENT_VERSION: u32 = 1;

/// Nonce length in bytes required by the existing AES-256-GCM payload format (12); not an arbitrary tuning value.
pub(crate) const AES_GCM_NONCE_SIZE: usize = 12;

// --- Logging ---

/// Maximum number of structured log records retained in the shared memory cache.
pub const LOG_CACHE_CAPACITY: usize = 5_000;

/// UTC timestamp format used by console and structured logs.
pub(crate) const LOG_TIMESTAMP_FORMAT: &[time::format_description::BorrowedFormatItem<'static>] =
    time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");

// --- REST lifecycle ---

/// Internal peer-address header overwritten by the trusted REST accept loop.
pub(crate) const REST_PEER_ADDR_HEADER: &str = "x-arachnea-peer-addr";

/// Maximum duration to drain in-flight connections during REST listener shutdown.
pub(crate) const REST_ACCEPT_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Maximum duration to join the REST accept-loop thread after shutdown.
pub(crate) const REST_STOP_GRACE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// Interval between main-thread dispatcher shutdown checks.
pub(crate) const MAIN_THREAD_POLL_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(100);

/// Interval between shutdown checks on the macOS process main thread.
#[cfg(target_os = "macos")]
pub(crate) const REST_SHUTDOWN_POLL_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(100);

/// Default loopback bind address for REST controllers.
pub(crate) const REST_DEFAULT_BIND_IP: std::net::IpAddr =
    std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);

/// Fallback API mount segment when a REST configuration does not provide one.
pub(crate) const REST_DEFAULT_API_SEGMENT: &str = "api";

/// Interval between REST supervisor shutdown/join checks.
pub(crate) const REST_SUPERVISOR_POLL_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(20);

// --- REST tray contracts ---

/// Custom URI scheme serving the dedicated REST log window.
pub(crate) const REST_LOG_WINDOW_SCHEME: &str = "arachnea-log";

/// Stable identifier of the REST server tray icon.
pub(crate) const REST_TRAY_ID: &str = "server-tray";

/// Tray command identifier for server shutdown.
pub(crate) const REST_MENU_SHUTDOWN: &str = "shutdown";

/// Tray command identifier for opening logs.
pub(crate) const REST_MENU_SHOW_LOGS: &str = "show-logs";

/// Tray command identifier for opening the public frontend.
pub(crate) const REST_MENU_OPEN_BROWSER: &str = "open-browser";

/// Tray command identifier for opening administration.
pub(crate) const REST_MENU_OPEN_ADMIN: &str = "open-admin";

/// Tray command identifier for reloading configuration.
pub(crate) const REST_MENU_RELOAD: &str = "reload-configuration";

/// Tray command identifier for server information.
pub(crate) const REST_MENU_SERVER_INFO: &str = "server-info";

/// Tray command identifier for network information.
pub(crate) const REST_MENU_NETWORK_INFO: &str = "network-info";

/// Stable label of the dedicated REST log window.
pub(crate) const REST_LOG_WINDOW_LABEL: &str = "logs";

/// Embedded log-window page; its content is maintained as a local HTML asset.
pub(crate) const REST_LOG_WINDOW_HTML: &str = include_str!("../resources/log_window.html");

// --- Tauri defaults and contracts ---

/// Fallback custom scheme of the Tauri controller; distinct from application-option defaults.
pub(crate) const TAURI_CONTROLLER_DEFAULT_WEB_SCHEME: &str = "arachnea";

/// Normalized fallback API prefix for Tauri stream routes, including surrounding slashes.
pub(crate) const TAURI_CONTROLLER_DEFAULT_API_PREFIX: &str = "/api/";

/// Absolute mount base used by Tauri frontend HTML.
pub(crate) const TAURI_WEB_BASE: &str = "/";

/// Label of the native administration window.
pub(crate) const TAURI_ADMIN_WINDOW_LABEL: &str = "admin";

/// Invoke command opening or refocusing administration.
pub(crate) const TAURI_ADMIN_WINDOW_OPEN_COMMAND: &str = "open_admin_window";

/// Default administration bundle route; the empty route opens the bundle root.
pub(crate) const TAURI_ADMIN_WINDOW_DEFAULT_ROUTE: &str = "";

/// Mount segment of the administration bundle.
pub(crate) const TAURI_ADMIN_WEB_MOUNT: &str = "admin";

/// Default presentation for public frontend links opened in a new browsing context.
pub const DEFAULT_TAURI_BROWSING_MODE: crate::controler::tauri::TauriBrowsingMode =
    crate::controler::tauri::TauriBrowsingMode::Tabs;

/// Logical-pixel height reserved for the native desktop tab strip.
pub(crate) const TAURI_TAB_STRIP_HEIGHT: f64 = 44.0;

/// Event name carrying the authoritative desktop tab snapshot.
pub(crate) const TAURI_TABS_CHANGED_EVENT: &str = "desktop-tabs-changed";

/// Initial administration window width in logical pixels.
pub(crate) const TAURI_ADMIN_WINDOW_WIDTH: f64 = 980.0;

/// Initial administration window height in logical pixels.
pub(crate) const TAURI_ADMIN_WINDOW_HEIGHT: f64 = 680.0;

// --- SQLite limits and metadata ---

/// Reserved SQLite table recording per-entity-table column metadata; changing it affects existing databases.
#[cfg(feature = "sqlite-persistence")]
pub(crate) const SQLITE_COLUMNS_META_TABLE: &str = "arachnea_columns";

/// Reserved SQLite table recording managed index metadata; changing it affects existing databases.
#[cfg(feature = "sqlite-persistence")]
pub(crate) const SQLITE_INDEXES_META_TABLE: &str = "arachnea_indexes";

/// Maximum entity keys grouped into one SQLite SELECT.
#[cfg(feature = "sqlite-persistence")]
pub(crate) const SQLITE_READ_BATCH_KEYS: usize = 32;

/// Maximum bound parameters per batched SELECT; wider keys keep the scalar path.
#[cfg(feature = "sqlite-persistence")]
pub(crate) const SQLITE_READ_MAX_PARAMETERS: usize = 900;

// --- Encoding and resource contracts ---

/// Base62 digit order used by persisted and externally visible identifiers; changing it changes encoded values.
pub(crate) const BASE62_ALPHABET: &[u8] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Linux installation prefixes recognized as packaged resources.
#[cfg(target_os = "linux")]
pub(crate) const LINUX_PACKAGED_PREFIXES: [&str; 4] = ["/usr/", "/opt/", "/snap/", "/tmp/.mount_"];

// --- Native window and listener runtime defaults ---

/// Delay before retrying a failed REST listener accept operation.
pub(crate) const REST_ACCEPT_RETRY_DELAY: std::time::Duration =
    std::time::Duration::from_millis(50);

/// Maximum duration to shut down the REST listener runtime after draining connections.
pub(crate) const REST_RUNTIME_SHUTDOWN_TIMEOUT: std::time::Duration =
    std::time::Duration::from_millis(500);

/// Initial log-window width in logical pixels.
pub(crate) const REST_LOG_WINDOW_WIDTH: f64 = 720.0;

/// Initial log-window height in logical pixels.
pub(crate) const REST_LOG_WINDOW_HEIGHT: f64 = 480.0;

/// Initial independent public frontend window width in logical pixels.
pub(crate) const TAURI_FRONTEND_WINDOW_WIDTH: f64 = 980.0;

/// Initial independent public frontend window height in logical pixels.
pub(crate) const TAURI_FRONTEND_WINDOW_HEIGHT: f64 = 680.0;
