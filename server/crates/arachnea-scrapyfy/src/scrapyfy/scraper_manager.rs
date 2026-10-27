use super::*;
use arachnea_core::controler::ControlerService;

use crate::SCRAPER_LOGGER_TARGETS as LOGGERS;

/// Contract exposing mutable access to the shared scraper aggregator.
pub trait ScraperManager {
    /// Returns a mutable reference to the underlying query aggregator.
    fn get_scraper_agregator_mut(&mut self) -> &mut ScraperAgregator;

    /// Creates a scraper `HttpClient` configured with the available proxy core.
    ///
    /// The default implementation creates a standalone client without proxy core.
    /// Concrete implementations that own a proxy core should override this method.
    fn create_http_client(&self, http_config: ScraperHttpConfig) -> HttpClient {
        HttpClient::with_http_config(http_config)
    }

    /// Registers the scraper's functions with the provided controler service.
    ///
    /// # Arguments
    ///
    /// * `controler` - The controler service to register the functions with.
    fn register_service(self, controler: &mut dyn ControlerService)
    where
        Self: Sized;

    /// Sets default log levels for carte dependencies.
    /// Must be called before `init_logger()` to take effect.
    fn init_sub_logger_levels() {
        for logger in LOGGERS {
            arachnea_core::logger::set_logger_levels(
                logger,
                Level::ERROR,
                Level::WARN,
                Level::INFO,
                Level::INFO,
                Level::TRACE,
            );
        }
    }

    /// Sets the log levels for carte dependencies.
    /// Must be called before `init_logger()` to take effect.
    ///
    /// # Arguments
    /// * `target` - The logger target name
    /// * `level` - Level to use
    ///
    /// # Examples
    ///
    /// ScraperManager::init_sub_logger_level(
    ///     "my_crate",
    ///     Level::INFO
    /// );
    /// ```
    fn init_sub_logger_level(level: Level) {
        for logger in LOGGERS {
            arachnea_core::logger::set_logger_level(logger, level);
        }
    }
}

use tracing::Level;

#[cfg(any(test, feature = "test-support"))]
#[path = "../../tests/support/scraper_manager/router.rs"]
mod test_router;
#[cfg(any(test, feature = "test-support"))]
pub use test_router::init_mock;
#[cfg(any(test, feature = "test-support"))]
use test_router::{MOCK_DATA_FOLDER, TEST_DATA_FOLDER};

#[cfg(any(test, feature = "test-support"))]
/// Test helpers used by scraper query integration tests.
#[path = "../../tests/support/scraper_manager/queries.rs"]
pub mod tests;
