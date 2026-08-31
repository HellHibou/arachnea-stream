//! Stable route facade with validated, atomic stream scraper reloads.
//!
//! Routes are registered once against [`ReloadableStreamScraper`]; every
//! request resolves the active [`StreamScraper`] instance at call time.
//! Reloads build and validate a replacement off the request path and swap it
//! in only when validation succeeds, producing a detailed report.

use anyhow::{Context, Result};
use serde::Serialize;
use std::sync::{Arc, RwLock};

use arachnea_core::controler::{ControlerService, ControlerServiceExt};
use arachnea_core::persistence::{CredentialsStore, PersistenceStore};
use arachnea_proxy::core::ArachneaProxyCore;
use arachnea_scrapyfy::{
    load_service_catalog_detailed, source_params_from_entries, PersistenceSourceEnabled,
    ScraperSourceDescriptor, ScraperSourceEnabled, ServiceCatalogFailureReason,
};

use crate::stream_scraper::{
    category_sources_from_request, StreamScraper, StreamScraperBuildOptions, GetBannersRequest,
    GetCategoryRequest, GetEntryRequest, GetLiveRequest, GetPlayersRequest, GetSectionRequest,
    GetSeasonRequest, GetServiceRequest, GetStreamRequest, ListLivesRequest, LoadHomeRequest,
    SearchRequest, DRM_LICENSE_PROXY_COMMAND, STREAM_SERVICES_STORE_NAME,
};

/// Endpoints captured when routes are first registered.
///
/// Reloaded instances reuse them so public proxy and DRM license paths stay
/// stable across swaps, including the shared proxy core backing the generic
/// `proxy` stream command.
#[derive(Clone)]
pub(crate) struct RegistrationEndpoints {
    /// Public path for DRM license proxy requests.
    pub(crate) drm_license_public_path: String,
    /// Public path for the generic HTTP proxy stream command.
    pub(crate) http_proxy_public_path: Option<String>,
    /// Proxy core shared with the registered `proxy` stream command.
    pub(crate) proxy_core: Option<ArachneaProxyCore>,
}

/// One source skipped during a reload, with the reason it was skipped.
#[derive(Clone, Debug, Serialize)]
pub struct StreamReloadSkippedSource {
    /// Parsed identifier, when available.
    pub id: Option<String>,
    /// YAML file path involved.
    pub path: String,
}

/// One source whose reload failed, with the error context.
#[derive(Clone, Debug, Serialize)]
pub struct StreamReloadSourceError {
    /// Parsed identifier, when available.
    pub id: Option<String>,
    /// YAML file path involved.
    pub path: String,
    /// Human-readable error message.
    pub message: String,
}

/// Detailed outcome of one reload attempt.
#[derive(Clone, Debug, Default, Serialize)]
pub struct StreamReloadReport {
    /// Whether the new instance replaced the active one.
    pub applied: bool,
    /// Service identifiers loaded in the replacement instance.
    pub loaded: Vec<String>,
    /// Service identifiers left disabled by their activation state.
    pub disabled: Vec<String>,
    /// Enabled sources skipped because their YAML file is missing.
    pub ignored: Vec<StreamReloadSkippedSource>,
    /// Sources that could not be validated or loaded.
    pub errors: Vec<StreamReloadSourceError>,
    /// Build failure message when the replacement could not be constructed.
    pub build_error: Option<String>,
}

/// Stable facade holding the active [`StreamScraper`] instance.
///
/// All controller routes resolve the active instance at request time, so a
/// successful reload swaps the instance without interrupting in-flight
/// requests or re-registering routes.
pub struct ReloadableStreamScraper {
    options: RwLock<StreamScraperBuildOptions>,
    registration: RwLock<Option<RegistrationEndpoints>>,
    inner: RwLock<Arc<StreamScraper>>,
}

impl ReloadableStreamScraper {
    /// Creates the facade and builds the initial instance from the options.
    ///
    /// # Arguments
    /// * `options` - Build options describing stores, manifest path, and sizing.
    ///
    /// # Returns
    /// A reloadable facade with the initial instance active.
    ///
    /// # Errors
    /// Returns an error when the initial instance cannot be built.
    pub fn new(options: StreamScraperBuildOptions) -> Result<Self> {
        let instance = StreamScraper::from_options(&options)?;
        Ok(Self {
            options: RwLock::new(options),
            registration: RwLock::new(None),
            inner: RwLock::new(Arc::new(instance)),
        })
    }

    /// Wraps an already-built instance, mainly for direct registrations.
    ///
    /// # Arguments
    /// * `scraper` - Active instance to expose.
    /// * `options` - Options that would rebuild an equivalent instance.
    pub(crate) fn from_instance(
        scraper: StreamScraper,
        options: StreamScraperBuildOptions,
    ) -> Self {
        Self {
            options: RwLock::new(options),
            registration: RwLock::new(None),
            inner: RwLock::new(Arc::new(scraper)),
        }
    }

    /// Returns the currently active scraper instance.
    pub fn current(&self) -> Arc<StreamScraper> {
        Arc::clone(&self.inner.read().expect("stream scraper lock poisoned"))
    }

    /// Returns the shared persistence store used for activation overrides.
    pub fn persistence_store(&self) -> Arc<dyn PersistenceStore> {
        self.options
            .read()
            .expect("stream scraper options lock poisoned")
            .persistence_store
            .clone()
    }

    /// Returns the shared credentials store used by service resolvers.
    pub fn credentials_store(&self) -> Arc<dyn CredentialsStore> {
        self.options
            .read()
            .expect("stream scraper options lock poisoned")
            .credentials_store
            .clone()
    }

    /// Stores the explicit local country used by geo proxy decisions.
    ///
    /// The country is remembered and reapplied to every reloaded instance.
    ///
    /// # Arguments
    /// * `country` - ISO country code for the current outbound location.
    pub async fn set_current_country(&self, country: impl AsRef<str>) {
        let country = country.as_ref().to_string();
        self.options
            .write()
            .expect("stream scraper options lock poisoned")
            .current_country = Some(country.clone());
        self.current().set_current_country(country).await;
    }

    /// Prepares endpoints and registers every stream route against this facade.
    ///
    /// Consumes the facade and returns it shared, ready for later reloads.
    ///
    /// # Arguments
    /// * `controler` - Controller receiving the stream routes.
    ///
    /// # Returns
    /// The facade wrapped in an `Arc`, as bound by the registered routes.
    pub fn register_service(mut self, controler: &mut dyn ControlerService) -> Arc<Self> {
        {
            let inner = self.inner.get_mut().expect("stream scraper lock poisoned");
            let scraper = Arc::get_mut(inner)
                .expect("stream scraper must not be shared before registration");
            scraper.prepare_registration(controler);
            let endpoints = RegistrationEndpoints {
                drm_license_public_path: scraper
                    .player_resolver_endpoints
                    .drm_license_public_path
                    .clone(),
                http_proxy_public_path: scraper
                    .player_resolver_endpoints
                    .http_proxy_public_path
                    .clone(),
                proxy_core: scraper.proxy_http_core.clone(),
            };
            *self
                .registration
                .get_mut()
                .expect("registration endpoints lock poisoned") = Some(endpoints);
        }

        let shared = Arc::new(self);
        register_routes(&shared, controler);
        shared
    }

    /// Builds and validates a replacement instance, then swaps it in on success.
    ///
    /// The sequence is: catalog inventory, activation-state synchronization
    /// without overwriting existing overrides, replacement construction off the
    /// request path, validation, and finally the atomic swap. The active
    /// instance keeps serving requests while the replacement is built.
    ///
    /// # Returns
    /// A [`StreamReloadReport`] detailing loaded, disabled, ignored, and
    /// failing services. `applied` is `false` when validation or construction
    /// failed, in which case the active instance is left untouched.
    ///
    /// # Errors
    /// Returns an error only for unexpected internal failures such as a fatal
    /// manifest resolution error or a panicked build task.
    pub async fn reload(&self) -> Result<StreamReloadReport> {
        let options = self
            .options
            .read()
            .expect("stream scraper options lock poisoned")
            .clone();
        let catalog = load_service_catalog_detailed(&options.services_config_path).with_context(
            || format!("Failed to reload service catalog {}.", options.services_config_path),
        )?;

        let policy = PersistenceSourceEnabled::new(
            Arc::clone(&options.persistence_store),
            STREAM_SERVICES_STORE_NAME,
        );
        let descriptors: Vec<ScraperSourceDescriptor> = catalog
            .entries
            .iter()
            .map(|entry| entry.source.clone())
            .collect();
        policy
            .register_defaults(&descriptors)
            .await
            .context("Failed to synchronize service activation defaults during reload.")?;

        let mut report = StreamReloadReport::default();
        for entry in &catalog.entries {
            match policy.is_enabled(&entry.source).await {
                Ok(true) => report.loaded.push(entry.source.id.clone()),
                Ok(false) => report.disabled.push(entry.source.id.clone()),
                Err(error) => report.errors.push(StreamReloadSourceError {
                    id: Some(entry.source.id.clone()),
                    path: entry.source.path.display().to_string(),
                    message: format!("Failed to read activation state: {error:#}"),
                }),
            }
        }
        for failure in &catalog.failures {
            match failure.reason {
                ServiceCatalogFailureReason::Missing => {
                    report.ignored.push(StreamReloadSkippedSource {
                        id: failure.id.clone(),
                        path: failure.path.display().to_string(),
                    });
                }
                ServiceCatalogFailureReason::Invalid | ServiceCatalogFailureReason::Duplicate => {
                    report.errors.push(StreamReloadSourceError {
                        id: failure.id.clone(),
                        path: failure.path.display().to_string(),
                        message: failure.message.clone(),
                    });
                }
            }
        }

        // Build the replacement off the request path; the active instance
        // keeps serving while this runs.
        let build_options = options.clone();
        let built =
            tokio::task::spawn_blocking(move || StreamScraper::from_options(&build_options))
                .await
                .map_err(|error| anyhow::anyhow!("Reload build task failed: {error}"))?;

        match built {
            Ok(mut scraper) if report.errors.is_empty() => {
                if let Some(endpoints) = self
                    .registration
                    .read()
                    .expect("registration endpoints lock poisoned")
                    .as_ref()
                {
                    scraper.apply_registration_endpoints(endpoints);
                }
                if let Some(country) = &options.current_country {
                    scraper.set_current_country(country).await;
                }
                *self.inner.write().expect("stream scraper lock poisoned") = Arc::new(scraper);
                report.applied = true;
                tracing::info!(
                    loaded = report.loaded.len(),
                    disabled = report.disabled.len(),
                    ignored = report.ignored.len(),
                    "stream scraper reloaded"
                );
            }
            Ok(_) => {
                report.applied = false;
                tracing::warn!(
                    errors = report.errors.len(),
                    "stream scraper reload refused: validation failed"
                );
            }
            Err(error) => {
                report.applied = false;
                report.build_error = Some(format!("{error:#}"));
                tracing::error!(error = %error, "stream scraper reload failed");
            }
        }

        Ok(report)
    }

    /// Blocking variant of [`ReloadableStreamScraper::reload`] for callers
    /// outside an async runtime, such as tray callbacks.
    ///
    /// # Errors
    /// Returns an error when the reload fails or the worker thread panics.
    pub fn reload_blocking(self: &Arc<Self>) -> Result<StreamReloadReport> {
        let shared = Arc::clone(self);
        std::thread::spawn(move || {
            tokio::runtime::Runtime::new()
                .map_err(anyhow::Error::from)?
                .block_on(shared.reload())
        })
        .join()
        .map_err(|_| anyhow::anyhow!("stream scraper reload thread panicked"))?
    }
}

/// Registers every stream route so each request resolves the active instance.
fn register_routes(
    reloadable: &Arc<ReloadableStreamScraper>,
    controler: &mut dyn ControlerService,
) {
    controler.register_result_function_with_state(
        "search",
        Arc::clone(reloadable),
        |reloadable, context, input: SearchRequest| async move {
            reloadable
                .current()
                .search(
                    context,
                    input.query,
                    input.media_types,
                    input.themes,
                    input.page,
                    source_params_from_entries(input.source_params),
                )
                .await
        },
    );

    controler.register_result_function_with_state(
        "load_home",
        Arc::clone(reloadable),
        |reloadable, context, _input: LoadHomeRequest| async move {
            reloadable.current().load_home(context).await
        },
    );

    controler.register_result_function_with_state(
        "get_service",
        Arc::clone(reloadable),
        |reloadable, context, _input: GetServiceRequest| async move {
            reloadable.current().get_service(context).await
        },
    );

    controler.register_result_function_with_state(
        "list_lives",
        Arc::clone(reloadable),
        |reloadable, context, _input: ListLivesRequest| async move {
            reloadable.current().list_lives(context).await
        },
    );

    controler.register_result_function_with_state(
        "get_category",
        Arc::clone(reloadable),
        |reloadable, context, input: GetCategoryRequest| async move {
            reloadable
                .current()
                .get_category(
                    context,
                    category_sources_from_request(input.source, input.sources),
                    input.page,
                    source_params_from_entries(input.source_params),
                )
                .await
        },
    );

    controler.register_result_function_with_state(
        "get_section",
        Arc::clone(reloadable),
        |reloadable, context, input: GetSectionRequest| async move {
            reloadable
                .current()
                .get_section(
                    context,
                    input.source,
                    input.link,
                    input.page,
                    source_params_from_entries(input.source_params),
                )
                .await
        },
    );

    controler.register_result_function_with_state(
        "get_banners",
        Arc::clone(reloadable),
        |reloadable, context, input: GetBannersRequest| async move {
            reloadable
                .current()
                .get_banners(context, input.source, input.link)
                .await
        },
    );

    controler.register_result_function_with_state(
        "get_players",
        Arc::clone(reloadable),
        |reloadable, context, input: GetPlayersRequest| async move {
            reloadable
                .current()
                .get_players(context, input.source, input.link)
                .await
        },
    );

    controler.register_result_function_with_state(
        "get_entry",
        Arc::clone(reloadable),
        |reloadable, context, input: GetEntryRequest| async move {
            reloadable
                .current()
                .get_entry(context, input.source, input.entry)
                .await
        },
    );

    controler.register_result_function_with_state(
        "get_season",
        Arc::clone(reloadable),
        |reloadable, context, input: GetSeasonRequest| async move {
            reloadable
                .current()
                .get_season(context, input.source, input.season, input.page)
                .await
        },
    );

    controler.register_result_function_with_state(
        "get_live",
        Arc::clone(reloadable),
        |reloadable, context, input: GetLiveRequest| async move {
            reloadable
                .current()
                .get_live(context, input.source, input.channel)
                .await
        },
    );

    controler.register_result_function_with_state(
        "get_stream",
        Arc::clone(reloadable),
        |reloadable, _context, input: GetStreamRequest| async move {
            reloadable
                .current()
                .get_stream(input.resolver, input.target)
                .await
                .map(|result| (result, None::<String>))
        },
    );

    controler.register_stream_function_with_state(
        DRM_LICENSE_PROXY_COMMAND,
        Arc::clone(reloadable),
        |reloadable, input| async move { reloadable.current().get_drm_license(input).await },
    );
}