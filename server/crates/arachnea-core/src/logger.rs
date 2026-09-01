//! Logging initialization helpers for the backend.
//!
//! The module installs the global `tracing` subscriber and exposes profile-aware
//! macros for overriding the default fallback log level before initialization.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::env;
use std::io;
use std::sync::{Arc, LazyLock, Mutex};
use time::macros::format_description;
use time::OffsetDateTime;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::{fmt, prelude::*, EnvFilter, Layer};

/// Re-export used by exported logging macros.
#[doc(hidden)]
pub use tracing;

/// The default log level before logger initialization.
#[cfg(debug_assertions)]
pub static mut DEFAULT_LEVEL: Level = Level::DEBUG; // Cargo run / development mode.

/// The default log level before logger initialization.
///
/// Release builds (`cargo build --release`) fall back to `INFO` while debug
/// builds use `DEBUG`.
#[cfg(not(debug_assertions))]
pub static mut DEFAULT_LEVEL: Level = Level::INFO;

/// Logger configuration with multiple level thresholds.
///
/// This struct allows configuring different log levels for different targets
/// based on the global log level setting.
#[derive(Clone)]
pub struct LoggerConfig {
    /// The target module or crate name for this logger configuration.
    target: String,
    /// The log level to use when the global level is ERROR.
    error_level: Level,
    /// The log level to use when the global level is WARN.
    warn_level: Level,
    /// The log level to use when the global level is INFO.
    info_level: Level,
    /// The log level to use when the global level is DEBUG.
    debug_level: Level,
    /// The log level to use when the global level is TRACE.
    trace_level: Level,
}

impl LoggerConfig {
    /// Creates a new LoggerConfig with the specified target and levels.
    ///
    /// # Arguments
    /// * `target` - The target module or crate name.
    /// * `error_level` - The log level to use when global level is ERROR.
    /// * `warn_level` - The log level to use when global level is WARN.
    /// * `info_level` - The log level to use when global level is INFO.
    /// * `debug_level` - The log level to use when global level is DEBUG.
    /// * `trace_level` - The log level to use when global level is TRACE.
    ///
    /// # Returns
    /// A new LoggerConfig instance.
    pub fn new(
        target: impl Into<String>,
        error_level: Level,
        warn_level: Level,
        info_level: Level,
        debug_level: Level,
        trace_level: Level,
    ) -> Self {
        Self {
            target: target.into(),
            error_level,
            warn_level,
            info_level,
            debug_level,
            trace_level,
        }
    }

    /// Returns the appropriate log level based on the global level.
    ///
    /// # Arguments
    /// * `global_level` - The current global log level.
    ///
    /// # Returns
    /// The corresponding configured log level for the given global level.
    fn get_level_for_global(&self, global_level: Level) -> Level {
        match global_level {
            Level::ERROR => self.error_level,
            Level::WARN => self.warn_level,
            Level::INFO => self.info_level,
            Level::DEBUG => self.debug_level,
            Level::TRACE => self.trace_level,
        }
    }
}

/// All managed loggers.
///
/// This static variable holds a thread-safe collection of LoggerConfig instances
/// that have been registered before logger initialization.
static LOGGERS: LazyLock<Mutex<Vec<LoggerConfig>>> = LazyLock::new(|| Mutex::new(Vec::new()));

/// Sets the default log level in debug builds before logger initialization.
///
/// The macro expects a `tracing::Level` variant name, such as `INFO` or `DEBUG`.
/// It changes the fallback level used by [`init_logger`] when `RUST_LOG` is not
/// set or cannot be parsed. In release builds, the macro expands to a no-op.
///
/// # Examples
///
/// ```
/// arachnea_core::logger::set_default_log_level_debug!(INFO);
/// arachnea_core::logger::init_logger();
/// ```
#[cfg(debug_assertions)]
#[macro_export]
macro_rules! set_default_log_level_debug {
    ($level:ident) => {
        unsafe {
            $crate::logger::DEFAULT_LEVEL = $crate::logger::tracing::Level::$level;
        }
    };
}

/// Sets the default log level in debug builds before logger initialization.
///
/// No-op variant used in release builds; see the `debug_assertions` variant
/// for details.
#[cfg(not(debug_assertions))]
#[macro_export]
macro_rules! set_default_log_level_debug {
    ($level:ident) => {};
}

pub use set_default_log_level_debug;

/// Sets the default log level in release builds before logger initialization.
///
/// The macro expects a `tracing::Level` variant name, such as `INFO` or `WARN`.
/// It changes the fallback level used by [`init_logger`] when `RUST_LOG` is not
/// set or cannot be parsed. In debug builds, the macro expands to a no-op.
///
/// # Examples
///
/// ```
/// arachnea_core::logger::set_default_log_level_release!(WARN);
/// arachnea_core::logger::init_logger();
/// ```
#[cfg(debug_assertions)]
#[macro_export]
macro_rules! set_default_log_level_release {
    ($level:ident) => {};
}

/// Sets the default log level in release builds before logger initialization.
///
/// The macro expects a `tracing::Level` variant name, such as `INFO` or `WARN`.
/// It changes the fallback level used by [`init_logger`] when `RUST_LOG` is not
/// set or cannot be parsed. In debug builds, the macro expands to a no-op.
///
/// # Examples
///
/// ```
/// arachnea_core::logger::set_default_log_level_release!(WARN);
/// arachnea_core::logger::init_logger();
/// ```
#[cfg(not(debug_assertions))]
#[macro_export]
macro_rules! set_default_log_level_release {
    ($level:ident) => {
        unsafe {
            $crate::logger::DEFAULT_LEVEL = $crate::logger::tracing::Level::$level;
        }
    };
}

pub use set_default_log_level_release;

/// Maximum number of log records retained in the in-memory log cache.
///
/// The log cache is a bounded ring buffer: when its length reaches this value,
/// the oldest record is dropped so that the most recent [`LOG_CACHE_CAPACITY`]
/// records are always kept.
pub const LOG_CACHE_CAPACITY: usize = 5_000;

/// A structured log record captured from a `tracing` event.
///
/// This is the structured form stored in the shared [`LogCache`] and exposed to
/// the dedicated log window. It carries the fields needed to render a readable
/// table row (timestamp, level, target, message) without relying on parsing the
/// free-form console text or stripping ANSI codes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogRecord {
    /// UTC timestamp of the event, formatted as `YYYY-MM-DD HH:MM:SS`.
    pub timestamp: String,
    /// The log level of the event, uppercase (e.g. `"INFO"`).
    pub level: String,
    /// The target (module path) the event was emitted from.
    pub target: String,
    /// The formatted message; falls back to the remaining fields when no
    /// `message` field is present.
    pub message: String,
}

/// Subscriber callback invoked for every new log record pushed to a [`LogCache`].
///
/// Consumers such as the dedicated log window register a subscriber to receive
/// live log records in addition to reading the historical buffer via
/// [`LogCache::snapshot`].
type LogSubscriber = Box<dyn Fn(&LogRecord) + Send + Sync + 'static>;

/// In-memory ring buffer of the most recent structured log records.
///
/// The cache is fed by a `tracing` layer (see [`LogCaptureLayer`]) and keeps at
/// most [`LOG_CACHE_CAPACITY`] records. It also forwards every new record to the
/// registered subscribers so a live consumer (for example the dedicated log
/// window) can be updated without polling the whole buffer.
pub struct LogCache {
    /// The bounded buffer holding the most recent structured log records.
    buffer: Mutex<VecDeque<LogRecord>>,
    /// Live subscribers notified for each new log record.
    subscribers: Mutex<Vec<LogSubscriber>>,
}

impl LogCache {
    /// Creates an empty log cache.
    ///
    /// # Returns
    /// A new empty [`LogCache`].
    pub fn new() -> Self {
        Self {
            buffer: Mutex::new(VecDeque::new()),
            subscribers: Mutex::new(Vec::new()),
        }
    }

    /// Appends a structured log record to the cache and notifies live subscribers.
    ///
    /// If the buffer already holds [`LOG_CACHE_CAPACITY`] records, the oldest
    /// record is removed before the new one is appended.
    ///
    /// # Arguments
    /// * `record` - The structured log record to store.
    pub fn push(&self, record: LogRecord) {
        {
            let mut buffer = self.buffer.lock().expect("log cache buffer poisoned");
            if buffer.len() >= LOG_CACHE_CAPACITY {
                buffer.pop_front();
            }
            buffer.push_back(record.clone());
        }
        let subscribers = self
            .subscribers
            .lock()
            .expect("log cache subscribers poisoned");
        for subscriber in subscribers.iter() {
            subscriber(&record);
        }
    }

    /// Returns a snapshot of all currently buffered log records, oldest first.
    ///
    /// # Returns
    /// A clone of the buffered records, ordered from oldest to newest.
    pub fn snapshot(&self) -> Vec<LogRecord> {
        self.buffer
            .lock()
            .expect("log cache buffer poisoned")
            .iter()
            .cloned()
            .collect()
    }

    /// Returns the number of log records currently buffered.
    ///
    /// # Returns
    /// The current number of buffered log records.
    pub fn len(&self) -> usize {
        self.buffer.lock().expect("log cache buffer poisoned").len()
    }

    /// Returns whether the log cache currently holds no records.
    ///
    /// # Returns
    /// `true` if the cache is empty, `false` otherwise.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Registers a live subscriber notified for each new log record.
    ///
    /// The subscriber is invoked synchronously on the thread that produced the
    /// log event. Multiple subscribers may be registered.
    ///
    /// # Arguments
    /// * `subscriber` - Callback invoked with each new structured log record.
    pub fn add_subscriber(&self, subscriber: impl Fn(&LogRecord) + Send + Sync + 'static) {
        self.subscribers
            .lock()
            .expect("log cache subscribers poisoned")
            .push(Box::new(subscriber));
    }

    /// Removes all live subscribers.
    ///
    /// New log lines are still stored in the buffer, but no longer forwarded.
    pub fn clear_subscribers(&self) {
        self.subscribers
            .lock()
            .expect("log cache subscribers poisoned")
            .clear();
    }
}

impl Default for LogCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Process-wide singleton log cache shared by all log consumers.
static GLOBAL_LOG_CACHE: LazyLock<Arc<LogCache>> = LazyLock::new(|| Arc::new(LogCache::new()));

/// Returns the process-wide shared log cache.
///
/// # Returns
/// A cloned `Arc` to the global [`LogCache`] instance.
pub fn global_log_cache() -> Arc<LogCache> {
    Arc::clone(&GLOBAL_LOG_CACHE)
}

/// UTC timestamp format shared by the console output and structured log records.
const TIMESTAMP_FORMAT: &[time::format_description::BorrowedFormatItem<'static>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");

/// Builds the UTC timestamp provider used by log formatting.
///
/// # Returns
/// The UTC timestamp provider matching the backend's log format.
fn build_log_timer() -> impl fmt::time::FormatTime {
    fmt::time::UtcTime::new(TIMESTAMP_FORMAT)
}

/// Visitor collecting the debug-rendered fields of a `tracing` event.
///
/// Each field is stored as a `(name, value)` pair so the message can be
/// reconstructed without relying on the console formatter.
#[derive(Default)]
struct FieldCollector {
    /// The collected field name/value pairs, in field order.
    fields: Vec<(String, String)>,
}

impl FieldCollector {
    /// Builds the message text for the collected fields.
    ///
    /// The `message` field value is used as the leading text. Any remaining
    /// fields are appended as `name=value` pairs, mirroring the console
    /// formatter so structured fields (e.g. `engine=... method=... url=...`)
    /// are not lost. When no `message` field is present, only the remaining
    /// fields are joined.
    ///
    /// # Returns
    /// The message string; empty only when no field was collected.
    fn into_message(self) -> String {
        if self.fields.is_empty() {
            return String::new();
        }

        let message = self
            .fields
            .iter()
            .find(|(name, _)| name == "message")
            .map(|(_, value)| value.clone());

        let mut tail = self
            .fields
            .iter()
            .filter(|(name, _)| name != "message")
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join(" ");

        match message {
            Some(message) => {
                if tail.is_empty() {
                    message
                } else {
                    tail.insert_str(0, &format!("{message} "));
                    tail
                }
            }
            None => tail,
        }
    }
}

impl Visit for FieldCollector {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.fields
            .push((field.name().to_string(), format!("{value:?}")));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields
            .push((field.name().to_string(), value.to_string()));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields
            .push((field.name().to_string(), value.to_string()));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .push((field.name().to_string(), value.to_string()));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .push((field.name().to_string(), value.to_string()));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.fields
            .push((field.name().to_string(), value.to_string()));
    }
}

/// `tracing` layer that captures structured [`LogRecord`]s into a [`LogCache`].
///
/// Unlike the console `fmt` layer, this layer does not render free-form text:
/// it builds a [`LogRecord`] (timestamp, level, target, message) for every
/// event and pushes it into the shared cache, so consumers never see ANSI
/// codes and can render a structured table.
struct LogCaptureLayer {
    /// The cache receiving the structured records.
    cache: Arc<LogCache>,
}

impl<S> Layer<S> for LogCaptureLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let timestamp = OffsetDateTime::now_utc()
            .format(&TIMESTAMP_FORMAT)
            .unwrap_or_default();
        let level = metadata.level().as_str().to_string();
        let target = metadata.target().to_string();

        let mut collector = FieldCollector::default();
        event.record(&mut collector);
        let message = collector.into_message();

        self.cache.push(LogRecord {
            timestamp,
            level,
            target,
            message,
        });
    }
}

/// Initializes the global `tracing` subscriber used by the backend.
///
/// The logger reads filtering directives from the standard `RUST_LOG`
/// environment variable. When no valid directive is provided, it defaults to
/// `debug` in debug builds and `info` in release builds. Log records are
/// formatted with UTC timestamps.
///
/// Two layers are installed:
/// - a console `fmt` layer that renders the free-form colored text to `stdout`
///   (preserving the original console behavior);
/// - a [`LogCaptureLayer`] that stores structured [`LogRecord`]s in the shared
///   log cache for the dedicated log window.
///
/// # Panics
///
/// Panics if a global tracing subscriber has already been installed.
pub fn init_logger() {
    // Retrieve the default level from the static variable (unsafe)
    let global_level: Level = unsafe { DEFAULT_LEVEL };

    // Build the EnvFilter: if RUST_LOG is present and valid, use it (priority).
    // Otherwise, fall back to the default level.
    let base_filter = match EnvFilter::try_from_default_env() {
        Ok(f) => f, // RUST_LOG present and valid -> priority
        Err(_) => EnvFilter::new(global_level.as_str()),
    };

    // Add overrides only if RUST_LOG doesn't already mention the target
    let mut filter = base_filter;
    let vec = LOGGERS.lock().expect("LOGGERS mutex poisoned");
    for config in vec.iter() {
        if !rust_log_mentions_target(&config.target) {
            // Map the global level to the appropriate level for this logger
            let mapped_level = config.get_level_for_global(global_level);
            let directive = format!("{}={}", config.target, mapped_level);
            filter = filter.add_directive(directive.parse().expect("invalid directive"));
        }
    }

    // Console layer keeps the original colored text output on stdout.
    let timer = build_log_timer();
    let console_layer = fmt::layer().with_timer(timer).with_writer(io::stdout);

    // Structured layer captures records into the shared in-memory log cache so
    // the dedicated log window can replay the last LOG_CACHE_CAPACITY records.
    let cache = global_log_cache();
    let capture_layer = LogCaptureLayer { cache };

    tracing_subscriber::registry()
        .with(console_layer)
        .with(capture_layer)
        .with(filter)
        .init();
}

/// Sets the log level for a specific logger.
/// Must be called before `init_logger()` to take effect.
///
/// # Arguments
/// * `target` - The logger target name
/// * `level` - Level to use
///
/// # Examples
/// ```
/// use tracing::Level;
///
/// arachnea_core::logger::set_logger_level(
///     "my_crate",
///     Level::INFO
/// );
/// ```
pub fn set_logger_level(target: impl Into<String>, level: Level) {
    set_logger_levels(target, level, level, level, level, level);
}

/// Sets the log levels for a specific logger.
/// Must be called before `init_logger()` to take effect.
///
/// # Arguments
/// * `target` - The logger target name
/// * `error_level` - Level to use when global level is ERROR
/// * `warn_level` - Level to use when global level is WARN
/// * `info_level` - Level to use when global level is INFO
/// * `debug_level` - Level to use when global level is DEBUG
/// * `trace_level` - Level to use when global level is TRACE
///
/// # Examples
/// ```
/// use tracing::Level;
///
/// arachnea_core::logger::set_logger_levels(
///     "my_crate",
///     Level::ERROR,
///     Level::WARN,
///     Level::INFO,
///     Level::DEBUG,
///     Level::TRACE,
/// );
/// ```
pub fn set_logger_levels(
    target: impl Into<String>,
    error_level: Level,
    warn_level: Level,
    info_level: Level,
    debug_level: Level,
    trace_level: Level,
) {
    let config = LoggerConfig::new(
        target,
        error_level,
        warn_level,
        info_level,
        debug_level,
        trace_level,
    );
    let mut vec = LOGGERS.lock().expect("LOGGERS mutex poisoned");
    vec.push(config);
}

/// Checks if RUST_LOG already mentions the target.
///
/// Splits RUST_LOG on ',' and spaces and compares the tokens.
///
/// # Arguments
/// * `target` - The target name to search for in RUST_LOG.
///
/// # Returns
/// `true` if the target is mentioned in RUST_LOG, `false` otherwise.
fn rust_log_mentions_target(target: &str) -> bool {
    if let Ok(rust_log) = env::var("RUST_LOG") {
        let t = target.trim().to_lowercase();
        rust_log
            .split(|c| c == ',' || c == ' ')
            .map(|s| s.trim().to_lowercase())
            .any(|token| {
                token == t
                    || token.starts_with(&format!("{}=", t))
                    || token.starts_with(&format!("{}:", t))
                    || token.starts_with(&format!("crate::{}=", t))
            })
    } else {
        false
    }
}
