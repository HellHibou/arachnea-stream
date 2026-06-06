/// Shared credentials storage contracts and helpers.
pub mod credentials_store;
/// AES-GCM encrypted credentials store used for server-side persistence.
pub mod encrypted_file_credentials_store;
/// Clear-text JSON credentials store used for local development and tests.
pub mod file_credentials_store;

/// Variuos ressources (application path, ...)
pub mod resources;

pub use credentials_store::CredentialsStore;
pub use encrypted_file_credentials_store::EncryptedFileCredentialsStore;
pub use file_credentials_store::FileCredentialsStore;
