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
/// SQLite-backed typed entity store with schema evolution.
#[cfg(feature = "sqlite-persistence")]
pub mod sqlite_store;
/// Generic asynchronous persistence contract for application records.
pub mod store;
/// Schema-declared, entity-typed persistence contracts and backends.
pub mod typed_store;

pub use credentials_store::CredentialsStore;
pub use encrypted_file_credentials_store::EncryptedFileCredentialsStore;
pub use file_codec::{JsonPersistenceFileCodec, PersistenceFileCodec};
pub use file_credentials_store::FileCredentialsStore;
pub use file_store::{LegacyFilePersistenceStore, PersistenceFileDocument};
pub use memory_store::LegacyMemoryPersistenceStore;
pub use store::{PersistedRecord, PersistenceKey, PersistenceStore, PersistenceTransaction};
pub use typed_store::{
    EntityKey, EntityQuery, EntityReader, EntitySchema, EntityWriter, Field, FieldRole, FieldType,
    FileEntityStore, LegacyTypedEntityStore, MemoryEntityStore, PersistenceStoreConfig,
    PersistentEntity, TypedEntityStore,
};

/// Typed SQLite persistence store configured for exactly one entity schema.
#[cfg(feature = "sqlite-persistence")]
pub use sqlite_store::SqliteEntityStore;
/// Typed SQLite persistence store configured for exactly one entity schema.
#[cfg(feature = "sqlite-persistence")]
pub type SqlitePersistenceStore<E> = SqliteEntityStore<E>;

/// Typed file persistence store configured for exactly one entity schema.
pub type FilePersistenceStore<E, C = JsonPersistenceFileCodec> = FileEntityStore<E, C>;
/// Typed in-memory persistence store configured for exactly one entity schema.
pub type MemoryPersistenceStore<E> = MemoryEntityStore<E>;

pub(crate) use store::{record_matches_filters, PersistenceBackend};
