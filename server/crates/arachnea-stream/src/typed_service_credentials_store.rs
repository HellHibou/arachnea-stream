//! `CredentialsStore` adapter over the typed `arachnea-services` store.
//!
//! Login and password are encrypted independently with AES-256-GCM before
//! being written into the `SourceServiceRecord` entity fields
//! (`base64(nonce || ciphertext)` per field). The activation override stored
//! on the same record is preserved on every credential write.

use anyhow::{bail, Result};
use std::future::Future;
use std::sync::Arc;

use arachnea_core::persistence::credentials_store::StoredCredentials;
use arachnea_core::persistence::{
    decrypt_value, encrypt_value, CredentialsStore, TypedEntityStore,
};
use arachnea_scrapyfy::SourceServiceRecord;

/// Credentials store persisting encrypted credentials in the typed
/// `arachnea-services` entity store.
pub struct TypedServiceCredentialsStore {
    store: Arc<dyn TypedEntityStore<SourceServiceRecord>>,
    encryption_key: [u8; 32],
}

impl TypedServiceCredentialsStore {
    /// Creates a credentials store over a typed service-record entity store.
    ///
    /// # Arguments
    /// * `store` - Typed store holding the `arachnea-services` records.
    /// * `encryption_key` - Static AES-256-GCM key applied to each field.
    ///
    /// # Returns
    /// A credentials store encrypting login and password independently.
    pub fn new(
        store: Arc<dyn TypedEntityStore<SourceServiceRecord>>,
        encryption_key: [u8; 32],
    ) -> Self {
        Self {
            store,
            encryption_key,
        }
    }

    /// Blocks on one async store operation, reusing the ambient runtime when
    /// available.
    fn wait<F>(&self, future: F) -> Result<F::Output>
    where
        F: Future,
    {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => Ok(tokio::task::block_in_place(|| handle.block_on(future))),
            Err(_) => Ok(tokio::runtime::Runtime::new()?.block_on(future)),
        }
    }

    /// Reads one service record from the typed store.
    fn read_record(&self, service_id: &str) -> Result<Option<SourceServiceRecord>> {
        let store = Arc::clone(&self.store);
        let service_id = service_id.to_string();
        self.wait(store.get(&service_id))?
    }

    /// Writes one service record to the typed store.
    fn write_record(&self, record: SourceServiceRecord) -> Result<()> {
        let store = Arc::clone(&self.store);
        self.wait(store.put(&record))?
    }
}

/// Trims and validates a service identifier.
fn normalize_service_id(service_id: &str) -> Result<String> {
    let service_id = service_id.trim().to_string();
    if service_id.is_empty() {
        bail!("Missing service identifier.");
    }
    Ok(service_id)
}

impl CredentialsStore for TypedServiceCredentialsStore {
    fn get_credentials(&self, service_id: &str) -> Result<Option<StoredCredentials>> {
        let service_id = normalize_service_id(service_id)?;
        let Some(record) = self.read_record(&service_id)? else {
            return Ok(None);
        };
        let (Some(login), Some(password)) = (record.login, record.password) else {
            return Ok(None);
        };

        Ok(Some(StoredCredentials {
            login: decrypt_value(&self.encryption_key, &login)?,
            password: decrypt_value(&self.encryption_key, &password)?,
        }))
    }

    fn set_credentials(&self, service_id: &str, credentials: StoredCredentials) -> Result<()> {
        let service_id = normalize_service_id(service_id)?;
        let login = credentials.login.trim();
        let password = credentials.password.trim();
        if login.is_empty() || password.is_empty() {
            bail!("Missing credentials login or password.");
        }

        // Read-modify-write keeps the activation override stored on the same
        // entity intact.
        let mut record = self.read_record(&service_id)?.unwrap_or_else(|| {
            // Defensive default: catalog services already get a record at
            // startup; an enabled default keeps an unregistered but
            // credential-configured service usable.
            SourceServiceRecord {
                source_id: service_id.clone(),
                enabled: true,
                login: None,
                password: None,
            }
        });
        record.login = Some(encrypt_value(&self.encryption_key, login)?);
        record.password = Some(encrypt_value(&self.encryption_key, password)?);
        self.write_record(record)
    }

    fn clear_credentials(&self, service_id: &str) -> Result<()> {
        let service_id = normalize_service_id(service_id)?;
        let Some(mut record) = self.read_record(&service_id)? else {
            return Ok(());
        };
        record.login = None;
        record.password = None;
        self.write_record(record)
    }
}
