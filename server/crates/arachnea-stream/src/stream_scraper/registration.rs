//! Controller registration and public endpoint synchronization.

use super::*;

impl StreamScraper {
    /// Prepares browser-facing endpoints and the core for the proxy command.
    ///
    /// Must be called once before route registration. Reloaded instances reuse
    /// the endpoints captured at registration time through
    /// [`StreamScraper::apply_registration_endpoints`].
    ///
    /// # Arguments
    /// * `controler` - Controller used to resolve public stream command paths.
    pub(crate) fn prepare_registration(&mut self, controler: &mut dyn ControlerService) {
        self.player_resolver_endpoints.drm_license_public_path =
            controler.stream_public_path(DRM_LICENSE_PROXY_COMMAND);

        let proxy_core = match self.proxy_http_core.clone() {
            Some(proxy_core) => Some(proxy_core),
            None => match ArachneaProxyCore::new(ProxyConfig::default()) {
                Ok(proxy_core) => Some(proxy_core),
                Err(error) => {
                    tracing::warn!(target: "arachnea_stream::stream_scraper",
                        error = %error,
                        "proxy_http handler not registered: failed to create default proxy core"
                    );
                    None
                }
            },
        };

        if let Some(proxy_core) = proxy_core.as_ref() {
            proxy_core.set_insecure_tls_hosts(
                self.scraper_agregator
                    .group_proxy_insecure_tls_hosts(STREAM_RESOLVER_GROUP_NAME),
            );
            let proxy_public_path = controler.stream_public_path(HTTP_PROXY_COMMAND);
            self.player_resolver_endpoints.http_proxy_public_path = Some(proxy_public_path);
        } else {
            tracing::warn!(target: "arachnea_stream::stream_scraper", "proxy_http handler not registered: no proxy core available");
        }

        self.proxy_http_core = proxy_core;
    }

    /// Applies registration-time endpoints to a rebuilt instance so reloaded
    /// scrapers keep serving the same public proxy and DRM license paths.
    ///
    /// # Arguments
    /// * `endpoints` - Endpoints captured when routes were first registered.
    pub(crate) fn apply_registration_endpoints(
        &mut self,
        endpoints: &crate::reloadable_stream_scraper::RegistrationEndpoints,
    ) {
        self.player_resolver_endpoints.drm_license_public_path =
            endpoints.drm_license_public_path.clone();
        self.player_resolver_endpoints.http_proxy_public_path =
            endpoints.http_proxy_public_path.clone();

        self.configure_proxy_insecure_tls_hosts();
    }
}
