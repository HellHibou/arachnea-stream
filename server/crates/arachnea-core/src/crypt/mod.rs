//! Cryptographic and hashing helpers shared by backend crates.

/// Base62 encoding primitives.
pub mod base62;

pub use base62::encode as encode_base62;