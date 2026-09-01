//! Persistence contracts and helpers for credentials storage and resource paths.
pub mod credentials_store;
/// AES-GCM encrypted credentials store used for server-side persistence.
pub mod encrypted_file_credentials_store;
/// Reusable codecs for file-backed persistence.
pub mod file_codec;
/// Clear-text JSON credentials store used for local development and tests.
pub mod file_credentials_store;
/// SQLite-backed typed entity store with schema evolution.
#[cfg(feature = "sqlite-persistence")]
pub mod sqlite_store;
/// Schema-declared, entity-typed persistence contracts and backends.
pub mod typed_store;
/// AES-GCM helpers encrypting single field values for typed entity stores.
pub mod value_encryption;

pub use credentials_store::CredentialsStore;
pub use encrypted_file_credentials_store::EncryptedFileCredentialsStore;
pub use file_codec::{JsonPersistenceFileCodec, PersistenceFileCodec};
pub use file_credentials_store::FileCredentialsStore;
pub use typed_store::{
    EntityKey, EntityQuery, EntityReader, EntitySchema, EntityWriter, Field, FieldRole, FieldType,
    FileEntityStore, MemoryEntityStore, PersistenceStoreConfig, PersistentEntity, TypedEntityStore,
};
pub use value_encryption::{decrypt_value, encrypt_value};

/// Typed SQLite persistence store configured for exactly one entity schema.
#[cfg(feature = "sqlite-persistence")]
pub use sqlite_store::{SqlKey, SqliteEntityStore};
/// Typed SQLite persistence store configured for exactly one entity schema.
#[cfg(feature = "sqlite-persistence")]
pub type SqlitePersistenceStore<E> = SqliteEntityStore<E>;

/// Typed file persistence store configured for exactly one entity schema.
pub type FilePersistenceStore<E, C = JsonPersistenceFileCodec> = FileEntityStore<E, C>;
/// Typed in-memory persistence store configured for exactly one entity schema.
pub type MemoryPersistenceStore<E> = MemoryEntityStore<E>;
