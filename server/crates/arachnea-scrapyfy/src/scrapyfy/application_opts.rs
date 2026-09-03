use anyhow::Context;
use arachnea_core::application::{ApplicationOptionDefinition, ApplicationOptionsProvider};
use arachnea_core::controler::options::CoreApplicationOptions;

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
    /// Whether `--server-port` was provided on the command line.
    pub server_port_specified: bool,
    /// Whether `--network` was provided on the command line.
    pub network_mode_specified: bool,
    /// Whether `--entrypoint-root` was provided on the command line.
    pub entrypoint_root_specified: bool,
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
}

impl Default for SrcapyfyApplicationOptions {
    fn default() -> Self {
        Self {
            application_option: CoreApplicationOptions::default(),
            current_country: None,
            cache_max_disk_bytes: None,
            cache_max_memory_bytes: None,
            server_port_specified: false,
            network_mode_specified: false,
            entrypoint_root_specified: false,
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

    fn from_vect(args: Vec<String>) -> anyhow::Result<Box<Self>> {
        let mut instance = SrcapyfyApplicationOptions::default()
            .with_application_option(*ApplicationOptionsProvider::from_vect(args.clone())?);

        let mut iter = args.iter();

        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--current-country" => {
                    instance.current_country = Some(
                        iter.next()
                            .context("Missing value for `--current-country`")?
                            .clone(),
                    );
                }
                "--cache-max-disk-bytes" => {
                    let value = iter
                        .next()
                        .context("Missing value for `--cache-max-disk-bytes`")?;

                    instance.cache_max_disk_bytes =
                        Some(value.parse::<u64>().with_context(|| {
                            format!("Invalid value for `--cache-max-disk-bytes`: `{value}`")
                        })?);
                }
                "--cache-max-memory-bytes" => {
                    let value = iter
                        .next()
                        .context("Missing value for `--cache-max-memory-bytes`")?;

                    instance.cache_max_memory_bytes =
                        Some(value.parse::<u64>().with_context(|| {
                            format!("Invalid value for `--cache-max-memory-bytes`: `{value}`")
                        })?);
                }
                "--server-port" => {
                    instance.server_port_specified = true;
                }
                "--network" => {
                    instance.network_mode_specified = true;
                }
                "--entrypoint-root" => {
                    instance.entrypoint_root_specified = true;
                }
                _ => {}
            }
        }

        Ok(Box::new(instance))
    }
}
