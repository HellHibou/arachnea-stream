use anyhow::Context;
use arachnea_core::application::{ApplicationOptionDefinition, ApplicationOptionsProvider};
use arachnea_core::controler::options::{CoreApplicationOptions, SettingSource};
use crate::scrapyfy::{ScraperAdminSettings, ScraperCacheConfig};
use std::collections::BTreeMap;

/// Runtime options parsed from command line arguments.
pub struct SrcapyfyApplicationOptions {
    /// Core application options shared by all Arachnea backends.
    pub application_option: CoreApplicationOptions,
    /// Explicit local country used for geo proxy decisions.
    pub current_country: Option<String>,
    /// Override of the maximum on-disk server cache size in bytes.
    pub cache_max_disk_bytes: Option<u64>,
    /// Override of the maximum in-memory server cache size in bytes.
    pub cache_max_memory_bytes: Option<u64>,
    /// Command-line-pinned scraper overrides only, used by the administration
    /// API to compute effective values and to reject edits on pinned fields.
    cli_scraper: ScraperAdminSettings,
}

impl SrcapyfyApplicationOptions {
    /// Sets the application options.
    ///
    /// # Arguments
    /// * `application_option` - The application options to set.
    ///
    /// # Returns
    /// The modified options for chaining.
    pub fn with_application_option(mut self, application_option: CoreApplicationOptions) -> Self {
        self.application_option = application_option;
        self
    }

    /// Sets the custom URI scheme used by the desktop frontend.
    ///
    /// # Arguments
    /// * `web_scheme` - The web scheme to use.
    ///
    /// # Returns
    /// The modified options for chaining.
    pub fn with_web_scheme(mut self, web_scheme: &str) -> Self {
        self.application_option.web_scheme = Some(web_scheme.to_string());
        self
    }

    /// Returns the command-line-pinned scraper settings.
    ///
    /// Only values explicitly provided through the command line are present; a
    /// `None` field means the persisted override (or the runtime default)
    /// applies.
    pub fn cli_scraper_settings(&self) -> &ScraperAdminSettings {
        &self.cli_scraper
    }

    /// Resolves the command-line cache overrides into an effective
    /// [`ScraperCacheConfig`].
    ///
    /// Returns `None` when no `--cache-max-*` option is present, so consumers
    /// keep the scraper runtime default cache configuration untouched. When
    /// only one limit is provided, the other limit falls back to its default.
    ///
    /// # Returns
    ///
    /// The effective cache configuration, or `None` when no override applies.
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

impl Default for SrcapyfyApplicationOptions {
    fn default() -> Self {
        Self {
            application_option: CoreApplicationOptions::default(),
            current_country: None,
            cache_max_disk_bytes: None,
            cache_max_memory_bytes: None,
            cli_scraper: ScraperAdminSettings::default(),
        }
    }
}

impl ApplicationOptionsProvider for SrcapyfyApplicationOptions {
    fn get_options(&self) -> Vec<ApplicationOptionDefinition> {
        let mut options = self.application_option.get_options();
        options.extend([
            ApplicationOptionDefinition::new(
                "--current-country",
                "Override the current country for the application.",
            ),
            ApplicationOptionDefinition::new(
                "--cache-max-disk-bytes",
                "Override the maximum disk cache size in bytes.",
            ),
            ApplicationOptionDefinition::new(
                "--cache-max-memory-bytes",
                "Override the maximum memory cache size in bytes.",
            ),
        ]);
        options
    }

    fn export(&self, output: &mut BTreeMap<String, Option<String>>) {
        self.application_option.export(output);
        if let Some(current_country) = &self.current_country {
            output.insert("current-country".to_string(), Some(current_country.clone()));
        }
        if let Some(cache_max_disk_bytes) = self.cache_max_disk_bytes {
            output.insert(
                "cache-max-disk-bytes".to_string(),
                Some(cache_max_disk_bytes.to_string()),
            );
        }
        if let Some(cache_max_memory_bytes) = self.cache_max_memory_bytes {
            output.insert(
                "cache-max-memory-bytes".to_string(),
                Some(cache_max_memory_bytes.to_string()),
            );
        }
    }

    fn parse_vect(&mut self, args: Vec<String>, source: SettingSource) -> anyhow::Result<()> {
        self.application_option.parse_vect(args.clone(), source)?;

        let mut iter = args.iter();

        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--current-country" => {
                    self.current_country = Some(
                        iter.next()
                            .context("Missing value for `--current-country`")?
                            .clone(),
                    );
                    if source == SettingSource::CommandLine {
                        self.cli_scraper.current_country = self.current_country.clone();
                    }
                }
                "--cache-max-disk-bytes" => {
                    let value = iter
                        .next()
                        .context("Missing value for `--cache-max-disk-bytes`")?;

                    self.cache_max_disk_bytes = Some(value.parse::<u64>().with_context(|| {
                        format!("Invalid value for `--cache-max-disk-bytes`: `{value}`")
                    })?);
                    if source == SettingSource::CommandLine {
                        self.cli_scraper.cache_max_disk_bytes = self.cache_max_disk_bytes;
                    }
                }
                "--cache-max-memory-bytes" => {
                    let value = iter
                        .next()
                        .context("Missing value for `--cache-max-memory-bytes`")?;

                    self.cache_max_memory_bytes =
                        Some(value.parse::<u64>().with_context(|| {
                            format!("Invalid value for `--cache-max-memory-bytes`: `{value}`")
                        })?);
                    if source == SettingSource::CommandLine {
                        self.cli_scraper.cache_max_memory_bytes = self.cache_max_memory_bytes;
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }
}
