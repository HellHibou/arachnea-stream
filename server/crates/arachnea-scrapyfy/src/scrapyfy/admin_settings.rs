//! Scrapyfy administrator overrides: current country and cache sizing.
//!
//! The overrides are persisted inside the shared application configuration
//! (`data/config.json`) through Core's generic `additional_options` map, so
//! Core never needs to know their semantics. This module owns everything
//! Scrapyfy specific: configuration keys, validation, command-line precedence
//! resolution, and conversion to the runtime cache configuration.
//! Command-line values always win over the persisted overrides.

use anyhow::{bail, Result};
use arachnea_core::controler::options::CoreApplicationOptions;

use super::ScraperCacheConfig;

/// Configuration key of the explicit current-country override.
pub const CURRENT_COUNTRY_KEY: &str = "current-country";
/// Configuration key of the maximum on-disk server cache size (bytes).
pub const CACHE_MAX_DISK_BYTES_KEY: &str = "cache-max-disk-bytes";
/// Configuration key of the maximum in-memory server cache size (bytes).
pub const CACHE_MAX_MEMORY_BYTES_KEY: &str = "cache-max-memory-bytes";

/// Administrator-managed scraper runtime overrides.
///
/// `None` fields keep the runtime default untouched (100 MiB disk / 32 MiB
/// memory for the cache, automatic detection for the current country).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScraperAdminSettings {
    /// Explicit current-country override (two-letter uppercase ISO code).
    pub current_country: Option<String>,
    /// Override of the maximum on-disk server cache size in bytes.
    pub cache_max_disk_bytes: Option<u64>,
    /// Override of the maximum in-memory server cache size in bytes.
    pub cache_max_memory_bytes: Option<u64>,
}

impl ScraperAdminSettings {
    /// Extracts the overrides persisted in the application configuration.
    ///
    /// Unparsable persisted values are ignored so a corrupted entry never
    /// prevents the application from starting.
    ///
    /// # Arguments
    /// * `configuration` - Persisted application configuration.
    pub fn from_configuration(configuration: &CoreApplicationOptions) -> Self {
        Self {
            current_country: configuration
                .additional_option(CURRENT_COUNTRY_KEY)
                .map(str::to_string),
            cache_max_disk_bytes: configuration
                .additional_option(CACHE_MAX_DISK_BYTES_KEY)
                .and_then(|value| value.parse().ok()),
            cache_max_memory_bytes: configuration
                .additional_option(CACHE_MAX_MEMORY_BYTES_KEY)
                .and_then(|value| value.parse().ok()),
        }
    }

    /// Validates every override value.
    ///
    /// # Errors
    /// Returns an error when the country is not a two-letter uppercase code or
    /// a cache size is zero.
    pub fn validate(&self) -> Result<()> {
        if let Some(country) = &self.current_country {
            validate_current_country(country)?;
        }
        for (name, value) in [
            (CACHE_MAX_DISK_BYTES_KEY, self.cache_max_disk_bytes),
            (CACHE_MAX_MEMORY_BYTES_KEY, self.cache_max_memory_bytes),
        ] {
            if value == Some(0) {
                bail!("`{name}` must be greater than zero.");
            }
        }
        Ok(())
    }

    /// Resolves the effective settings: the command line wins over the
    /// persisted override, which wins over the runtime default.
    ///
    /// # Arguments
    /// * `cli` - Command-line-pinned settings (`None` fields are unpinned).
    pub fn merged_overriding(self, cli: &ScraperAdminSettings) -> ScraperAdminSettings {
        ScraperAdminSettings {
            current_country: cli.current_country.clone().or(self.current_country),
            cache_max_disk_bytes: cli.cache_max_disk_bytes.or(self.cache_max_disk_bytes),
            cache_max_memory_bytes: cli
                .cache_max_memory_bytes
                .or(self.cache_max_memory_bytes),
        }
    }

    /// Writes these overrides into the application configuration.
    ///
    /// A `None` field removes the corresponding configuration key so the
    /// runtime default applies again.
    ///
    /// # Arguments
    /// * `configuration` - Configuration to update in place.
    pub fn apply_to_configuration(&self, configuration: &mut CoreApplicationOptions) {
        configuration.set_additional_option(CURRENT_COUNTRY_KEY, self.current_country.clone());
        configuration.set_additional_option(
            CACHE_MAX_DISK_BYTES_KEY,
            self.cache_max_disk_bytes.map(|bytes| bytes.to_string()),
        );
        configuration.set_additional_option(
            CACHE_MAX_MEMORY_BYTES_KEY,
            self.cache_max_memory_bytes.map(|bytes| bytes.to_string()),
        );
    }

    /// Resolves the effective cache configuration, or `None` when no override
    /// applies so consumers keep the runtime default.
    pub fn scraper_cache_config(&self) -> Option<ScraperCacheConfig> {
        if self.cache_max_disk_bytes.is_none() && self.cache_max_memory_bytes.is_none() {
            return None;
        }
        let mut config = ScraperCacheConfig::default();
        if let Some(bytes) = self.cache_max_disk_bytes {
            config.max_disk_bytes = bytes;
        }
        if let Some(bytes) = self.cache_max_memory_bytes {
            config.max_memory_bytes = bytes;
        }
        Some(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_resolve_no_cache_override() {
        let settings = ScraperAdminSettings::default();
        assert!(settings.scraper_cache_config().is_none());
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn cli_values_win_over_persisted_overrides() {
        let persisted = ScraperAdminSettings {
            current_country: Some("FR".to_string()),
            cache_max_disk_bytes: Some(1000),
            cache_max_memory_bytes: None,
        };
        let cli = ScraperAdminSettings {
            current_country: Some("DE".to_string()),
            cache_max_disk_bytes: None,
            cache_max_memory_bytes: Some(2000),
        };
        let effective = persisted.merged_overriding(&cli);
        assert_eq!(effective.current_country.as_deref(), Some("DE"));
        assert_eq!(effective.cache_max_disk_bytes, Some(1000));
        assert_eq!(effective.cache_max_memory_bytes, Some(2000));
    }

    #[test]
    fn country_validation_rejects_wrong_shape() {
        assert!(validate_current_country("FR").is_ok());
        assert!(validate_current_country("fr").is_err());
        assert!(validate_current_country("FRA").is_err());
        assert!(validate_current_country("F1").is_err());
        assert!(validate_current_country("").is_err());
    }

    #[test]
    fn settings_roundtrip_through_configuration() {
        let mut configuration = CoreApplicationOptions::default();
        let settings = ScraperAdminSettings {
            current_country: Some("FR".to_string()),
            cache_max_disk_bytes: Some(104_857_600),
            cache_max_memory_bytes: None,
        };
        settings.apply_to_configuration(&mut configuration);
        assert_eq!(
            ScraperAdminSettings::from_configuration(&configuration),
            settings
        );
    }

    #[test]
    fn clearing_removes_configuration_keys() {
        let mut configuration = CoreApplicationOptions::default();
        ScraperAdminSettings {
            current_country: Some("FR".to_string()),
            cache_max_disk_bytes: Some(1000),
            cache_max_memory_bytes: Some(2000),
        }
        .apply_to_configuration(&mut configuration);
        ScraperAdminSettings::default().apply_to_configuration(&mut configuration);
        assert_eq!(
            ScraperAdminSettings::from_configuration(&configuration),
            ScraperAdminSettings::default()
        );
    }
}

/// Validates one current-country override value.
///
/// # Errors
/// Returns an error when the value is not a two-letter uppercase ASCII code.
pub fn validate_current_country(country: &str) -> Result<()> {
    let valid = country.len() == 2 && country.bytes().all(|byte| byte.is_ascii_uppercase());
    if !valid {
        bail!(
            "Current country override must be a two-letter uppercase ISO code, got `{country}`."
        );
    }
    Ok(())
}
