use super::*;

fn ensure_test_data_dir() {
    crate::scrapyfy::scraper_agregator::tests::configure_test_application_data_dir();
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
    assert!(ScraperAdminSettings {
        cache_block_size_bytes: Some(4),
        ..ScraperAdminSettings::default()
    }
    .validate()
    .is_ok());
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
    assert!(ScraperAdminSettings {
        cache_max_memory_bytes: Some(8 * 1024),
        cache_block_size_bytes: Some(4 * 1024),
        ..ScraperAdminSettings::default()
    }
    .validate()
    .is_ok());
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
    assert_eq!(config.block_size_bytes, DEFAULT_CACHE_BLOCK_SIZE_KIB * 1024);
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
