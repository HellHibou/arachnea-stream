use super::eligibility::{epoch_millis, has_active_destination_cooldown, is_selection_candidate};
use super::records::prepare_provider_records;
use super::*;
#[cfg(feature = "persistence")]
use crate::core::proxy_repository::memory_proxy_store;
#[cfg(feature = "persistence")]
use crate::core::ProxyPreferenceRepository;
#[cfg(feature = "persistence")]
use crate::core::TypedProxyRepository;
use crate::core::{
    Destination, ProxyAvailabilityHint, ProxyBatchSink, ProxyCapabilityStatus, ProxyDeclaration,
    ProxyDestinationFailure, ProxyError, ProxyLoadRequest, ProxyProtocol, ProxyRuntimeStatus,
    ProxySearchContext, Result,
};
use anyhow::Result as AnyResult;
use async_trait::async_trait;
use futures::FutureExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

#[cfg(feature = "persistence")]
use crate::core::PROXY_CACHE_TTL;

use support::*;

#[path = "discovery.rs"]
mod discovery;
#[path = "observations.rs"]
mod observations;
#[path = "preferences.rs"]
mod preferences;
#[path = "probing.rs"]
mod probing;
#[path = "records.rs"]
mod records;
#[path = "selection.rs"]
mod selection;
#[path = "../../support/inventory.rs"]
mod support;
#[path = "../proxy_selection.rs"]
mod tokens;
