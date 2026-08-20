//! Persistence contracts and helpers for credentials storage and resource paths.
pub mod credentials_store;
/// AES-GCM encrypted credentials store used for server-side persistence.
pub mod encrypted_file_credentials_store;
/// Reusable codecs for file-backed persistence.
pub mod file_codec;
/// Clear-text JSON credentials store used for local development and tests.
pub mod file_credentials_store;
/// File-backed generic persistence store with atomic writes.
pub mod file_store;
/// In-memory generic persistence store used as a default and for tests.
pub mod memory_store;
/// Generic asynchronous persistence contract for application records.
pub mod store;

pub use credentials_store::CredentialsStore;
pub use encrypted_file_credentials_store::EncryptedFileCredentialsStore;
pub use file_codec::{JsonPersistenceFileCodec, PersistenceFileCodec};
pub use file_credentials_store::FileCredentialsStore;
pub use file_store::{FilePersistenceStore, PersistenceFileDocument};
pub use memory_store::MemoryPersistenceStore;
pub use store::{PersistedRecord, PersistenceKey, PersistenceStore};
