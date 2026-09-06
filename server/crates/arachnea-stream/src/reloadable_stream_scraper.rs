//! Stable route facade with atomic stream scraper reconstruction.
//!
//! Routes are registered once against [`ReloadableStreamScraper`]; every
//! request resolves the active [`StreamScraper`] instance at call time.
//! Validated rebuilds construct a replacement off the request path and swap it
//! in only when construction succeeds, producing a detailed report.

use anyhow::Result;
use std::sync::{Arc, RwLock};

use arachnea_core::controler::{ControlerService, ControlerServiceExt};
use arachnea_core::persistence::{CredentialsStore, TypedEntityStore};
use arachnea_proxy::core::ArachneaProxyCore;
use arachnea_scrapyfy::admin::RuntimeReloadReport;
use arachnea_scrapyfy::source_params_from_entries;
use arachnea_scrapyfy::{ScraperAdminSettings, ScraperRuntimeOptions, SourceServiceRecord};

use crate::stream_scraper::{
    category_sources_from_request, GetBannersRequest, GetCategoryRequest, GetEntryRequest,
    GetLiveRequest, GetPlayersRequest, GetSeasonRequest, GetSectionRequest, GetServiceRequest,
    GetStreamRequest, ListLivesRequest, LoadHomeRequest, SearchRequest, StreamScraper,
    StreamScraperBuildOptions, DRM_LICENSE_PROXY_COMMAND,
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

/// Stable facade holding the active [`StreamScraper`] instance.
///
/// All controller routes resolve the active instance at request time, so a
/// successful reload swaps the instance without interrupting in-flight
/// requests or re-registering routes.
pub struct ReloadableStreamScraper {
    options: RwLock<StreamScraperBuildOptions>,
    /// Scrapyfy runtime options (cache sizing) reapplied to every built
    /// instance, so reloads keep the effective configuration of the initial
    /// build.
    runtime_options: RwLock<ScraperRuntimeOptions>,
    registration: RwLock<Option<RegistrationEndpoints>>,
    inner: RwLock<Arc<StreamScraper>>,
}

impl ReloadableStreamScraper {
    /// Creates the facade and builds the initial instance from the options.
    ///
    /// # Arguments
    /// * `options` - Build options describing stores and manifest path.
    /// * `runtime_options` - Scrapyfy runtime options (cache sizing) applied to
    ///   the initial instance and to every reloaded instance.
    ///
    /// # Returns
    /// A reloadable facade with the initial instance active.
    ///
    /// # Errors
    /// Returns an error when the initial instance cannot be built.
    pub fn new(
        options: StreamScraperBuildOptions,
        runtime_options: ScraperRuntimeOptions,
    ) -> Result<Self> {
        let mut instance = StreamScraper::from_options(&options)?;
        runtime_options.apply_to(&mut instance.scraper_agregator);
        Ok(Self {
            options: RwLock::new(options),
            runtime_options: RwLock::new(runtime_options),
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
            runtime_options: RwLock::new(ScraperRuntimeOptions::default()),
            registration: RwLock::new(None),
            inner: RwLock::new(Arc::new(scraper)),
        }
    }

    /// Returns the currently active scraper instance.
    pub fn current(&self) -> Arc<StreamScraper> {
        Arc::clone(&self.inner.read().expect("stream scraper lock poisoned"))
    }

    /// Returns the typed store used for activation overrides.
    pub fn source_enabled_store(&self) -> Arc<dyn TypedEntityStore<SourceServiceRecord>> {
        self.options
            .read()
            .expect("stream scraper options lock poisoned")
            .stores
            .source_enabled
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

    /// Applies the effective scraper runtime overrides to the live runtime.
    ///
    /// The current country is remembered (and reapplied on later reloads) and
    /// the cache sizing is stored in the runtime options. When the cache
    /// sizing changes, the instance is rebuilt so the new bounds apply to the
    /// fresh aggregator.
    ///
    /// # Errors
    /// Returns an error when the replacement instance cannot be built.
    pub async fn apply_scraper_settings(&self, settings: &ScraperAdminSettings) -> Result<()> {
        let new_config = settings.scraper_cache_config();
        let cache_changed = {
            let current = self
                .runtime_options
                .read()
                .expect("stream scraper runtime options lock poisoned");
            match (current.cache_config.as_ref(), new_config.as_ref()) {
                (Some(left), Some(right)) => {
                    left.max_disk_bytes != right.max_disk_bytes
                        || left.max_memory_bytes != right.max_memory_bytes
                }
                (None, None) => false,
                _ => true,
            }
        };
        if cache_changed {
            self.runtime_options
                .write()
                .expect("stream scraper runtime options lock poisoned")
                .cache_config = new_config;
        }
        if let Some(country) = &settings.current_country {
            self.set_current_country(country).await;
        }
        if cache_changed {
            self.rebuild_validated().await?;
        }
        Ok(())
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
            let scraper =
                Arc::get_mut(inner).expect("stream scraper must not be shared before registration");
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

    /// Builds a replacement from a group already validated by Scrapyfy, then swaps it in.
    ///
    /// The sequence is replacement construction off the request path, restoration
    /// of registration-time endpoints and runtime options, then the atomic swap.
    /// The active instance keeps serving requests while the replacement is built.
    ///
    /// # Returns
    /// A [`RuntimeReloadReport`] stating whether the replacement instance was
    /// applied and, when applicable, the build failure context. `applied` is
    /// `false` when construction failed, in which case the active instance is
    /// left untouched.
    ///
    /// # Errors
    /// Returns an error only for unexpected internal failures such as a panicked
    /// build task.
    pub async fn rebuild_validated(&self) -> Result<RuntimeReloadReport> {
        let options = self
            .options
            .read()
            .expect("stream scraper options lock poisoned")
            .clone();

        // Build the replacement off the request path; the active instance
        // keeps serving while this runs.
        let build_options = options.clone();
        let built = tokio::task::spawn_blocking(move || {
            StreamScraper::from_validated_options(&build_options)
        })
        .await
        .map_err(|error| anyhow::anyhow!("Reload build task failed: {error}"))?;

        let mut report = RuntimeReloadReport {
            applied: false,
            build_error: None,
        };
        match built {
            Ok(mut scraper) => {
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
                self.runtime_options
                    .read()
                    .expect("stream scraper runtime options lock poisoned")
                    .apply_to(&mut scraper.scraper_agregator);
                *self.inner.write().expect("stream scraper lock poisoned") = Arc::new(scraper);
                report.applied = true;
                tracing::info!("stream scraper reloaded");
            }
            Err(error) => {
                report.applied = false;
                report.build_error = Some(format!("{error:#}"));
                tracing::error!(error = %error, "stream scraper reload failed");
            }
        }

        Ok(report)
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
