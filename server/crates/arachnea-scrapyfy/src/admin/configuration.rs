//! Shared administrator configuration with an application-supplied destination.

use anyhow::Result;
use arachnea_core::controler::options::CoreApplicationOptions;
use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};

/// Shared in-memory configuration, published only after a successful save.
pub struct AdminConfigurationStore {
    current: Arc<RwLock<CoreApplicationOptions>>,
    path: PathBuf,
}

impl AdminConfigurationStore {
    /// Creates a store without reading or writing the destination.
    ///
    /// # Arguments
    /// * `configuration` - Previously loaded persisted settings.
    /// * `path` - Application-owned configuration file path.
    pub fn new(configuration: CoreApplicationOptions, path: PathBuf) -> Self {
        Self {
            current: Arc::new(RwLock::new(configuration)),
            path,
        }
    }

    /// Returns a snapshot of the persisted settings.
    pub fn snapshot(&self) -> CoreApplicationOptions {
        self.current
            .read()
            .expect("configuration lock poisoned")
            .clone()
    }

    /// Returns the configuration shared with runtime settings resolvers.
    pub fn shared(&self) -> Arc<RwLock<CoreApplicationOptions>> {
        Arc::clone(&self.current)
    }

    /// Merges administrator-owned fields and saves them before publishing state.
    ///
    /// Unknown application options are retained. Callers serialize read/modify/write
    /// transactions with the administration mutation lock.
    ///
    /// # Arguments
    /// * `settings` - Updated settings, retaining the established option-map format.
    ///
    /// # Errors
    /// Returns validation, atomic persistence or file-permission failures.
    pub fn save(&self, settings: &CoreApplicationOptions) -> Result<()> {
        let mut config = self.current.write().expect("configuration lock poisoned");
        let mut candidate = config.clone();
        candidate.server_port = settings.server_port;
        candidate.entrypoint_root = settings.entrypoint_root.clone();
        candidate.network_mode = settings.network_mode;
        candidate.password_hash = settings.password_hash.clone();
        for (name, value) in &settings.additional_options {
            candidate
                .additional_options
                .insert(name.clone(), value.clone());
        }
        candidate.save(&self.path)?;
        *config = candidate;
        Ok(())
    }
}
