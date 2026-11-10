//! Budget and concurrency reservations for translation queries.

use crate::error::TranslationError;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Opaque reservation held while one provider request is in flight.
///
/// Dropping or releasing the reservation frees its concurrency slot. Consumed
/// quota is deliberately not refunded: a provider may have processed or counted
/// a request even when the caller observed a failure.
pub trait TranslationBudgetReservation: Send {
    /// Releases the reservation and its concurrency slot.
    fn release(self: Box<Self>);
}

/// Port reserving account budget and concurrency before a provider call.
///
/// Implementations must fail with [`TranslationError::BudgetExhausted`] when
/// the account cannot accept the request immediately. Waiting indefinitely is
/// not allowed.
#[async_trait]
pub trait TranslationBudgetPort: Send + Sync {
    /// Reports whether quota and concurrency are available without consuming them.
    ///
    /// # Arguments
    ///
    /// * `units` - Total planned units, excluding possible retries.
    fn available(&self, units: u64) -> bool;

    /// Reserves quota units and one concurrency slot.
    ///
    /// # Arguments
    ///
    /// * `units` - Provider units consumed by the request.
    ///
    /// # Errors
    ///
    /// Returns [`TranslationError::BudgetExhausted`] when quota or concurrency
    /// is unavailable.
    async fn reserve(
        &self,
        units: u64,
    ) -> Result<Box<dyn TranslationBudgetReservation>, TranslationError>;
}

/// Unlimited budget used when the caller does not inject an account budget.
pub struct UnlimitedTranslationBudget;

struct UnlimitedReservation;

impl TranslationBudgetReservation for UnlimitedReservation {
    fn release(self: Box<Self>) {}
}

#[async_trait]
impl TranslationBudgetPort for UnlimitedTranslationBudget {
    fn available(&self, _units: u64) -> bool {
        true
    }

    async fn reserve(
        &self,
        _units: u64,
    ) -> Result<Box<dyn TranslationBudgetReservation>, TranslationError> {
        Ok(Box::new(UnlimitedReservation))
    }
}

/// One in-memory account budget configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetLimit {
    /// Maximum concurrent provider requests. Must be at least one.
    pub max_concurrent_requests: usize,
    /// Quota units allowed per fixed window, or `None` for no quota limit.
    pub max_units_per_window: Option<u64>,
    /// Quota window duration in seconds. Must be at least one.
    pub window_seconds: u64,
}

impl BudgetLimit {
    /// Creates a validated budget limit.
    ///
    /// # Arguments
    ///
    /// * `max_concurrent_requests` - Positive concurrency bound.
    /// * `max_units_per_window` - Optional positive quota bound.
    /// * `window_seconds` - Positive quota window.
    ///
    /// # Errors
    ///
    /// Returns [`TranslationError::InvalidRequest`] for a zero bound.
    pub fn new(
        max_concurrent_requests: usize,
        max_units_per_window: Option<u64>,
        window_seconds: u64,
    ) -> Result<Self, TranslationError> {
        if max_concurrent_requests == 0 {
            return Err(TranslationError::InvalidRequest {
                detail: "translation concurrency must be at least one".to_string(),
            });
        }
        if max_units_per_window == Some(0) {
            return Err(TranslationError::InvalidRequest {
                detail: "translation quota must be positive when declared".to_string(),
            });
        }
        if window_seconds == 0 {
            return Err(TranslationError::InvalidRequest {
                detail: "translation quota window must be positive".to_string(),
            });
        }
        Ok(Self {
            max_concurrent_requests,
            max_units_per_window,
            window_seconds,
        })
    }

    /// Returns a conservative default budget.
    ///
    /// One request may run at a time and quota is unlimited by default; account
    /// quota is enabled explicitly from service configuration.
    pub fn unlimited_quota(max_concurrent_requests: usize) -> Self {
        Self {
            max_concurrent_requests: max_concurrent_requests.max(1),
            max_units_per_window: None,
            window_seconds: 1,
        }
    }
}

impl Default for BudgetLimit {
    fn default() -> Self {
        Self::unlimited_quota(1)
    }
}

/// In-memory token-window budget and reservation-backed concurrency limiter.
pub struct InMemoryTranslationBudget {
    state: Arc<Mutex<BudgetState>>,
}

struct BudgetState {
    limit: BudgetLimit,
    active_requests: usize,
    used_units: u64,
    window_started: Instant,
}

struct InMemoryReservation {
    state: Arc<Mutex<BudgetState>>,
}

impl Drop for InMemoryReservation {
    fn drop(&mut self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.active_requests = state.active_requests.saturating_sub(1);
    }
}

impl TranslationBudgetReservation for InMemoryReservation {
    fn release(self: Box<Self>) {}
}

impl InMemoryTranslationBudget {
    /// Creates an in-memory budget.
    ///
    /// # Arguments
    ///
    /// * `limit` - Validated concurrency and quota bounds.
    pub fn new(limit: BudgetLimit) -> Self {
        Self {
            state: Arc::new(Mutex::new(BudgetState {
                limit,
                active_requests: 0,
                used_units: 0,
                window_started: Instant::now(),
            })),
        }
    }

    /// Returns the configured limit.
    pub fn limit(&self) -> BudgetLimit {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .limit
    }

    /// Updates account limits while preserving consumed quota and active calls.
    ///
    /// # Arguments
    ///
    /// * `limit` - Replacement account limits after an administrator reload.
    pub fn update_limit(&self, limit: BudgetLimit) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .limit = limit;
    }
}

impl BudgetState {
    /// Refreshes the fixed quota window and checks the proposed reservation.
    ///
    /// # Arguments
    ///
    /// * `units` - Units required in addition to already consumed quota.
    fn available(&mut self, units: u64) -> bool {
        if self.window_started.elapsed() >= Duration::from_secs(self.limit.window_seconds) {
            self.used_units = 0;
            self.window_started = Instant::now();
        }
        self.active_requests < self.limit.max_concurrent_requests
            && self.used_units.checked_add(units).is_some_and(|next| {
                self.limit
                    .max_units_per_window
                    .is_none_or(|max| next <= max)
            })
    }
}

#[async_trait]
impl TranslationBudgetPort for InMemoryTranslationBudget {
    fn available(&self, units: u64) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .available(units)
    }

    async fn reserve(
        &self,
        units: u64,
    ) -> Result<Box<dyn TranslationBudgetReservation>, TranslationError> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.available(units) {
            return Err(TranslationError::BudgetExhausted {
                detail: "account quota or concurrency is unavailable".into(),
            });
        }
        state.used_units += units;
        state.active_requests += 1;
        Ok(Box::new(InMemoryReservation {
            state: self.state.clone(),
        }))
    }
}
