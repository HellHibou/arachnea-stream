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

use super::{ScraperCacheConfig, DEFAULT_CACHE_BLOCK_SIZE_KIB, DEFAULT_CACHE_MAX_DISK_KIB, DEFAULT_CACHE_MAX_MEMORY_KIB};

/// Configuration key of the explicit current-country override.
pub const CURRENT_COUNTRY_KEY: &str = "current-country";
/// Configuration key of the maximum on-disk server cache size, in K (kilobytes).
pub const CACHE_MAX_DISK_BYTES_KEY: &str = "cache-max-disk-bytes";
/// Configuration key of the maximum in-memory server cache size, in K (kilobytes).
pub const CACHE_MAX_MEMORY_BYTES_KEY: &str = "cache-max-memory-bytes";
/// Configuration key of the cache block size, in K (kilobytes).
pub const CACHE_BLOCK_SIZE_KEY: &str = "cache-block-size";

/// Cache sizing overrides expressed in K (kilobytes).
///
/// The public names keep the historical `_bytes` suffix; only the values are
/// expressed in kilobytes (see `docs/dev-tracking/cache-block-size-analysis.md`).
/// The conversion to bytes happens at the very last step, in
/// [`ScraperAdminSettings::scraper_cache_config`].
const KIB: u64 = 1024;

/// Administrator-managed scraper runtime overrides.
///
/// `None` fields keep the runtime default untouched (100 MiB disk / 32 MiB
/// memory / 16 MiB block size for the cache, automatic detection for the
/// current country).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScraperAdminSettings {
    /// Explicit current-country override (two-letter uppercase ISO code).
    pub current_country: Option<String>,
    /// Override of the maximum on-disk server cache size in K (kilobytes).
    pub cache_max_disk_bytes: Option<u64>,
    /// Override of the maximum in-memory server cache size in K (kilobytes).
    pub cache_max_memory_bytes: Option<u64>,
    /// Override of the cache block size in K (kilobytes).
    pub cache_block_size_bytes: Option<u64>,
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
            cache_block_size_bytes: configuration
                .additional_option(CACHE_BLOCK_SIZE_KEY)
                .and_then(|value| value.parse().ok()),
        }
    }

    /// Returns the effective cache bound in K, or the runtime default.
    fn effective_disk_kib(&self) -> u64 {
        self.cache_max_disk_bytes
            .unwrap_or(DEFAULT_CACHE_MAX_DISK_KIB)
    }

    /// Returns the effective memory bound in K, or the runtime default.
    fn effective_memory_kib(&self) -> u64 {
        self.cache_max_memory_bytes
            .unwrap_or(DEFAULT_CACHE_MAX_MEMORY_KIB)
    }

    /// Returns the effective block size in K, or the runtime default.
    fn effective_block_kib(&self) -> u64 {
        self.cache_block_size_bytes
            .unwrap_or(DEFAULT_CACHE_BLOCK_SIZE_KIB)
    }

    /// Validates every override value.
    ///
    /// # Errors
    /// Returns an error when the country is not a two-letter uppercase code, a
    /// cache size is zero, the block size is not a multiple of 4 K, or the
    /// block size is not strictly smaller than the effective disk and memory
    /// bounds (which fall back to their defaults when not overridden).
    pub fn validate(&self) -> Result<()> {
        if let Some(country) = &self.current_country {
            validate_current_country(country)?;
        }
        for (name, value) in [
            (CACHE_MAX_DISK_BYTES_KEY, self.cache_max_disk_bytes),
            (CACHE_MAX_MEMORY_BYTES_KEY, self.cache_max_memory_bytes),
            (CACHE_BLOCK_SIZE_KEY, self.cache_block_size_bytes),
        ] {
            if value == Some(0) {
                bail!("`{name}` must be greater than zero.");
            }
        }
        let block_kib = self.effective_block_kib();
        if block_kib % 4 != 0 {
            bail!(
                "`{CACHE_BLOCK_SIZE_KEY}` must be a multiple of 4 K, got `{block_kib}` K."
            );
        }
        let disk_kib = self.effective_disk_kib();
        if block_kib >= disk_kib {
            bail!(
                "`{CACHE_BLOCK_SIZE_KEY}` ({block_kib} K) must be smaller than the disk cache size ({disk_kib} K)."
            );
        }
        let memory_kib = self.effective_memory_kib();
        if block_kib >= memory_kib {
            bail!(
                "`{CACHE_BLOCK_SIZE_KEY}` ({block_kib} K) must be smaller than the memory cache size ({memory_kib} K)."
            );
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
            cache_block_size_bytes: cli
                .cache_block_size_bytes
                .or(self.cache_block_size_bytes),
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
            self.cache_max_disk_bytes.map(|kib| kib.to_string()),
        );
        configuration.set_additional_option(
            CACHE_MAX_MEMORY_BYTES_KEY,
            self.cache_max_memory_bytes.map(|kib| kib.to_string()),
        );
        configuration.set_additional_option(
            CACHE_BLOCK_SIZE_KEY,
            self.cache_block_size_bytes.map(|kib| kib.to_string()),
        );
    }

    /// Resolves the effective cache configuration, or `None` when no override
    /// applies so consumers keep the runtime default.
    ///
    /// This is the only place where the K (kilobyte) overrides are converted
    /// to bytes.
    pub fn scraper_cache_config(&self) -> Option<ScraperCacheConfig> {
        if self.cache_max_disk_bytes.is_none()
            && self.cache_max_memory_bytes.is_none()
            && self.cache_block_size_bytes.is_none()
        {
            return None;
        }
        let mut config = ScraperCacheConfig::default();
        if let Some(kib) = self.cache_max_disk_bytes {
            config.max_disk_bytes = kib * KIB;
        }
        if let Some(kib) = self.cache_max_memory_bytes {
            config.max_memory_bytes = kib * KIB;
        }
        if let Some(kib) = self.cache_block_size_bytes {
            config.block_size_bytes = kib * KIB;
        }
        Some(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ensure_test_data_dir() {
        let _ = arachnea_core::application::configure_application_data_dir_name(
            "arachnea-scrapyfy-tests",
        );
    }

    #[test]
    fn default_settings_resolve_no_cache_override() {
        ensure_test_data_dir();
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
            cache_block_size_bytes: None,
        };
        let cli = ScraperAdminSettings {
            current_country: Some("DE".to_string()),
            cache_max_disk_bytes: None,
            cache_max_memory_bytes: Some(2000),
            cache_block_size_bytes: None,
        };
        let effective = persisted.merged_overriding(&cli);
        assert_eq!(effective.current_country.as_deref(), Some("DE"));
        assert_eq!(effective.cache_max_disk_bytes, Some(1000));
        assert_eq!(effective.cache_max_memory_bytes, Some(2000));
    }

    #[test]
    fn block_size_must_be_a_multiple_of_four_kib() {
        let settings = ScraperAdminSettings {
            cache_block_size_bytes: Some(5),
            ..ScraperAdminSettings::default()
        };
        assert!(settings.validate().is_err());
        assert!(
            ScraperAdminSettings {
                cache_block_size_bytes: Some(4),
                ..ScraperAdminSettings::default()
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn block_size_must_stay_below_the_effective_bounds() {
        // 16 MiB block vs. an 8 MiB memory bound: rejected even though the
        // disk default (100 MiB) would allow it.
        let settings = ScraperAdminSettings {
            cache_max_memory_bytes: Some(8 * 1024),
            cache_block_size_bytes: Some(16 * 1024),
            ..ScraperAdminSettings::default()
        };
        assert!(settings.validate().is_err());
        // Lowering the block below the smallest bound fixes the configuration.
        assert!(
            ScraperAdminSettings {
                cache_max_memory_bytes: Some(8 * 1024),
                cache_block_size_bytes: Some(4 * 1024),
                ..ScraperAdminSettings::default()
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn cache_config_converts_kib_overrides_to_bytes() {
        ensure_test_data_dir();
        let settings = ScraperAdminSettings {
            cache_max_disk_bytes: Some(10 * 1024),
            cache_max_memory_bytes: Some(2 * 1024),
            cache_block_size_bytes: Some(1024),
            ..ScraperAdminSettings::default()
        };
        let config = settings
            .scraper_cache_config()
            .expect("overrides must resolve to a cache configuration");
        assert_eq!(config.max_disk_bytes, 10 * 1024 * 1024);
        assert_eq!(config.max_memory_bytes, 2 * 1024 * 1024);
        assert_eq!(config.block_size_bytes, 1024 * 1024);
    }

    #[test]
    fn defaults_match_the_kib_constants() {
        ensure_test_data_dir();
        let config = ScraperCacheConfig::default();
        assert_eq!(config.max_disk_bytes, DEFAULT_CACHE_MAX_DISK_KIB * 1024);
        assert_eq!(config.max_memory_bytes, DEFAULT_CACHE_MAX_MEMORY_KIB * 1024);
        assert_eq!(
            config.block_size_bytes,
            DEFAULT_CACHE_BLOCK_SIZE_KIB * 1024
        );
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
            cache_max_disk_bytes: Some(102_400),
            cache_max_memory_bytes: None,
            cache_block_size_bytes: None,
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
            cache_block_size_bytes: Some(16),
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
