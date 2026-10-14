//! Durable accepted histories with request-awaited, serialized commits.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, Weak};

use anyhow::Result;
use arachnea_core::persistence::{
    EntityQuery, EntityReader, EntitySchema, EntityWriter, Field, MemoryEntityStore,
    PersistenceStoreConfig, PersistentEntity, TypedEntityStore,
};
use async_trait::async_trait;
use tokio::sync::Mutex;

use super::{ProxyDestinationPreference, ProxyKey};

/// Separate namespace for exact-destination accepted histories.
pub const PROXY_PREFERENCE_STORE_NAME: &str = "proxy-preferences";

/// Persistence boundary for destination histories; implementations serialize commits.
#[async_trait]
pub trait ProxyPreferenceRepository: Send + Sync {
    /// Loads an exact destination without requiring provider discovery.
    async fn get(&self, destination: &str) -> Result<Option<ProxyDestinationPreference>>;
    /// Merges and commits history, preserving newer durable observations.
    async fn save(&self, preference: &ProxyDestinationPreference) -> Result<()>;
    /// Removes orphan references after an endpoint is evicted.
    async fn remove_endpoint(&self, endpoint: &ProxyKey) -> Result<()>;
}

/// Typed-store implementation with a shared commit lock, including file snapshots.
pub struct TypedProxyPreferenceRepository {
    store: Arc<dyn TypedEntityStore<ProxyDestinationPreference>>,
    commits: Arc<Mutex<()>>,
}

impl TypedProxyPreferenceRepository {
    /// Creates a repository. Share this instance between inventories using this store.
    ///
    /// # Arguments
    /// * `store` - Application-owned typed preference store.
    pub fn new(store: Arc<dyn TypedEntityStore<ProxyDestinationPreference>>) -> Self {
        static LOCKS: OnceLock<std::sync::Mutex<HashMap<usize, Weak<Mutex<()>>>>> = OnceLock::new();
        let identity = Arc::as_ptr(&store) as *const () as usize;
        let mut locks = LOCKS
            .get_or_init(Default::default)
            .lock()
            .expect("preference commit registry poisoned");
        locks.retain(|_, lock| lock.strong_count() > 0);
        let commits = locks
            .get(&identity)
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| {
                let lock = Arc::new(Mutex::new(()));
                locks.insert(identity, Arc::downgrade(&lock));
                lock
            });
        Self { store, commits }
    }
}

/// Creates the non-durable store used by compatibility constructors.
///
/// # Errors
/// Returns an error if the entity schema is invalid.
pub fn memory_proxy_preference_store(
) -> Result<Arc<dyn TypedEntityStore<ProxyDestinationPreference>>> {
    Ok(Arc::new(MemoryEntityStore::new(
        PersistenceStoreConfig::new(
            PROXY_PREFERENCE_STORE_NAME,
            ProxyDestinationPreference::schema(),
        )?,
    )?))
}

#[async_trait]
impl ProxyPreferenceRepository for TypedProxyPreferenceRepository {
    async fn get(&self, destination: &str) -> Result<Option<ProxyDestinationPreference>> {
        self.store.get(&destination.to_string()).await
    }

    async fn save(&self, preference: &ProxyDestinationPreference) -> Result<()> {
        let _guard = self.commits.lock().await;
        let mut merged = self
            .store
            .get(&preference.destination)
            .await?
            .unwrap_or_else(|| preference.clone());
        for item in &preference.endpoints {
            merged.record_success(item.endpoint.clone(), item.accepted_at);
        }
        self.store.put(&merged).await
    }

    async fn remove_endpoint(&self, endpoint: &ProxyKey) -> Result<()> {
        let _guard = self.commits.lock().await;
        for mut preference in self.store.find(&EntityQuery::new()).await? {
            let count = preference.endpoints.len();
            preference
                .endpoints
                .retain(|item| &item.endpoint != endpoint);
            if count != preference.endpoints.len() {
                if preference.endpoints.is_empty() {
                    self.store.delete(&preference.destination).await?;
                } else {
                    self.store.put(&preference).await?;
                }
            }
        }
        Ok(())
    }
}

impl PersistentEntity for ProxyDestinationPreference {
    type Key = String;
    fn key(&self) -> String {
        self.destination.clone()
    }
    fn schema() -> EntitySchema {
        EntitySchema::new()
            .primary_key(Field::string("destination"))
            .field(Field::json("endpoints"))
    }
    fn write_to(&self, writer: &mut EntityWriter) -> Result<()> {
        writer.string("destination", &self.destination)?;
        writer.json("endpoints", serde_json::to_value(&self.endpoints)?)?;
        Ok(())
    }
    fn read_from(reader: &EntityReader<'_>) -> Result<Self> {
        let mut preference = Self {
            destination: reader.string("destination")?.into(),
            endpoints: serde_json::from_value(reader.json("endpoints")?.clone())?,
        };
        preference.normalize();
        Ok(preference)
    }
}
