//! Logging initialization helpers for the backend.
//!
//! The module installs the global `tracing` subscriber and exposes profile-aware
//! macros for overriding the default fallback log level before initialization.

use std::env;
use std::sync::{LazyLock, Mutex};
use time::macros::format_description;
use tracing::Level;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Re-export used by exported logging macros.
#[doc(hidden)]
pub use tracing;

/// The default log level before logger initialization.
#[cfg(debug_assertions)]
pub static mut DEFAULT_LEVEL: Level = Level::DEBUG; // Cargo run / development mode.

#[cfg(not(debug_assertions))]
pub static mut DEFAULT_LEVEL: Level = Level::INFO; // Cargo build --release mode.

/// Logger configuration with multiple level thresholds
#[derive(Clone)]
pub struct LoggerConfig {
    target: String,
    error_level: Level,
    warn_level: Level,
    info_level: Level,
    debug_level: Level,
    trace_level: Level,
}

impl LoggerConfig {
    /// Creates a new LoggerConfig with the specified target and levels
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

    /// Returns the appropriate log level based on the global level
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

/// All managed loggers
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

/// Initializes the global `tracing` subscriber used by the backend.
///
/// The logger reads filtering directives from the standard `RUST_LOG`
/// environment variable. When no valid directive is provided, it defaults to
/// `debug` in debug builds and `info` in release builds. Log records are
/// formatted with UTC timestamps.
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

    // Format / timer setup
    let format = format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
    let timer = fmt::time::UtcTime::new(format);
    let layer = fmt::layer().with_timer(timer);

    tracing_subscriber::registry()
        .with(layer)
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
/// logger::set_logger_level(
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
/// logger::set_logger_levels(
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
/// Splits RUST_LOG on ',' and spaces and compares the tokens.
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
