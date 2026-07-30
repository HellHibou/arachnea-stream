#![warn(missing_docs)]
//! Shared Arachnea backend primitives.

/// Controller abstractions and REST/Tauri backends.
pub mod controler;

/// Persistence contracts and runtime resource helpers.
pub mod persistence;

/// Logging initialization helpers shared by backend crates.
pub mod logger;

/// Thread-safe error correlation code generator.
pub mod error_code;

/// Various application resources (application path, ...)
pub mod application;