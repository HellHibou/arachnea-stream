use std::sync::{Arc, RwLock};

/// Shared local-country state used to decide whether a requested geo-targeted
/// proxy country is already satisfied by the current outbound location.
#[derive(Clone, Default)]
pub struct SharedLocalCountry {
    inner: Arc<RwLock<LocalCountryState>>,
}

#[derive(Clone, Default)]
struct LocalCountryState {
    version: u64,
    explicit_country: Option<String>,
    detected_country: Option<String>,
}

/// Immutable view of the current local-country state.
#[derive(Clone, Default)]
pub struct LocalCountrySnapshot {
    /// Monotonic version incremented whenever local-country state changes.
    pub version: u64,
    /// Effective local country, preferring explicit configuration over detection.
    pub current_country: Option<String>,
}

impl LocalCountrySnapshot {
    /// Returns whether a requested proxy country should be bypassed for this snapshot.
    ///
    /// Empty requested countries or unknown local country never trigger a bypass.
    pub fn should_bypass_requested_proxy_country(
        &self,
        requested_country: impl AsRef<str>,
    ) -> bool {
        let requested_country = normalize_country(requested_country.as_ref());
        if requested_country.is_empty() {
            return false;
        }

        self.current_country
            .as_deref()
            .map(|local_country| local_country == requested_country)
            .unwrap_or(false)
    }
}

impl SharedLocalCountry {
    /// Creates an empty local-country state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores the explicitly configured local country.
    ///
    /// Empty values clear the explicit country. Stored values are normalized to
    /// uppercase ASCII after trimming whitespace.
    pub fn set_explicit_country(&self, country: impl AsRef<str>) {
        let country = normalize_country(country.as_ref());
        if let Ok(mut guard) = self.inner.write() {
            guard.version = guard.version.saturating_add(1);
            guard.explicit_country = (!country.is_empty()).then_some(country);
        }
    }

    /// Clears the explicitly configured local country.
    pub fn clear_explicit_country(&self) {
        if let Ok(mut guard) = self.inner.write() {
            guard.version = guard.version.saturating_add(1);
            guard.explicit_country = None;
        }
    }

    /// Stores the automatically detected local country.
    ///
    /// Empty values clear the detected country. Stored values are normalized to
    /// uppercase ASCII after trimming whitespace.
    pub fn set_detected_country(&self, country: impl AsRef<str>) {
        let country = normalize_country(country.as_ref());
        if let Ok(mut guard) = self.inner.write() {
            guard.version = guard.version.saturating_add(1);
            guard.detected_country = (!country.is_empty()).then_some(country);
        }
    }

    /// Clears the automatically detected local country.
    pub fn clear_detected_country(&self) {
        if let Ok(mut guard) = self.inner.write() {
            guard.version = guard.version.saturating_add(1);
            guard.detected_country = None;
        }
    }

    /// Returns the explicitly configured local country, if any.
    pub fn explicit_country(&self) -> Option<String> {
        self.inner
            .read()
            .ok()
            .and_then(|guard| guard.explicit_country.clone())
    }

    /// Returns the automatically detected local country, if any.
    pub fn detected_country(&self) -> Option<String> {
        self.inner
            .read()
            .ok()
            .and_then(|guard| guard.detected_country.clone())
    }

    /// Returns the effective local country, preferring explicit configuration
    /// over automatic detection.
    pub fn current_country(&self) -> Option<String> {
        self.snapshot().current_country
    }

    /// Returns a versioned snapshot of the effective local-country state.
    pub fn snapshot(&self) -> LocalCountrySnapshot {
        self.inner
            .read()
            .map(|guard| LocalCountrySnapshot {
                version: guard.version,
                current_country: guard
                    .explicit_country
                    .clone()
                    .or_else(|| guard.detected_country.clone()),
            })
            .unwrap_or_default()
    }

    /// Returns whether a requested proxy country should be bypassed.
    ///
    /// The decision is conservative: empty requested countries or unknown local
    /// country never trigger a bypass.
    pub fn should_bypass_requested_proxy_country(
        &self,
        requested_country: impl AsRef<str>,
    ) -> bool {
        let requested_country = normalize_country(requested_country.as_ref());
        if requested_country.is_empty() {
            return false;
        }

        self.snapshot()
            .should_bypass_requested_proxy_country(requested_country)
    }
}

fn normalize_country(country: &str) -> String {
    country.trim().to_ascii_uppercase()
}
