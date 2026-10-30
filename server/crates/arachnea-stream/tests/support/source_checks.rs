//! Strict v1 source-check loading and validation before scenario execution.

use anyhow::{ensure, Context, Result};
use arachnea_scrapyfy::scrapyfy::scraper_manager::tests::{
    assertions::{validate_path, Expectations},
    read_strict_yaml,
};
use arachnea_scrapyfy::scrapyfy::scraper_query_collection::ScraperQueryCollection;
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    path::{Component, Path, PathBuf},
};

use crate::{
    SOURCE_CHECK_MAX_STEPS, SOURCE_CHECK_MAX_STEP_TIMEOUT_SECONDS,
    SOURCE_CHECK_MAX_TIMEOUT_SECONDS, SOURCE_CHECK_SCHEMA_VERSION,
    SOURCE_CHECK_STEP_TIMEOUT_SECONDS, SOURCE_CHECK_TIMEOUT_SECONDS,
};

/// Independent expectations for exactly one production source.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SourceCheck {
    /// Supported test-only schema version.
    pub schema_version: u32,
    /// Exact production source identifier and fixture filename stem.
    pub source: String,
    /// Production YAML path relative to the supplied services root.
    pub source_config: String,
    /// Independently expected and explicitly absent capabilities.
    pub capabilities: Capabilities,
    /// Ordered operations and their assertions.
    pub steps: Vec<Step>,
    /// Bounded execution deadlines.
    #[serde(default)]
    pub limits: Limits,
    /// Required environment values, never printed or injected as credentials.
    #[serde(default)]
    pub prerequisites: Vec<Prerequisite>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Capabilities {
    /// Operations that must exist and have steps.
    pub expected: Vec<String>,
    /// Operations that must be absent and must not be executed.
    pub not_expected: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Step {
    /// Unique step identifier, used by dynamic input references.
    pub id: String,
    /// Generic facade operation, never a provider identifier.
    pub operation: Operation,
    /// Only the keys allowed for this operation may be supplied.
    #[serde(default)]
    pub inputs: HashMap<String, Input>,
    /// Useful-data minima and per-object field requirements.
    pub expect: Expectations,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Operation {
    /// Static metadata and supported capabilities.
    ServiceStreamMetadata,
    /// Home content or navigation.
    LoadHome,
    /// Search using an explicit reference query.
    Search,
    /// Content reached from a category URL.
    GetCategory,
    /// Content reached from a section URL.
    GetSection,
    /// One requested content detail.
    GetEntry,
    /// Player descriptors without playback qualification.
    GetPlayers,
    /// Episodes reached from a season URL or identifier.
    GetSeason,
    /// Isolated source's live channel catalog.
    ListLives,
    /// One live channel; query_url supplies the facade's channel argument.
    GetLive,
    /// Deferred banner descriptors.
    GetBanners,
    /// Deferred recommendation entries.
    GetRecommendations,
    /// Source-owned YAML stream resolution without playback qualification.
    ResolveStream,
    /// Legacy episode query without a dedicated Stream facade method.
    GetEpisode,
}

impl Operation {
    /// Returns the stable query name used by production and test YAML.
    pub fn name(self) -> &'static str {
        match self {
            Self::ServiceStreamMetadata => "service_stream_metadata",
            Self::LoadHome => "load_home",
            Self::Search => "search",
            Self::GetCategory => "get_category",
            Self::GetSection => "get_section",
            Self::GetEntry => "get_entry",
            Self::GetPlayers => "get_players",
            Self::GetSeason => "get_season",
            Self::ListLives => "list_lives",
            Self::GetLive => "get_live",
            Self::GetBanners => "get_banners",
            Self::GetRecommendations => "get_recommendations",
            Self::ResolveStream => "resolve_stream",
            Self::GetEpisode => "get_episode",
        }
    }

    /// Whether successful useful data contributes to source-health coverage.
    pub fn is_content(self) -> bool {
        !matches!(self, Self::ServiceStreamMetadata | Self::LoadHome)
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum Input {
    /// An explicit search term or query URL.
    Literal(String),
    /// Literal 1-based page, allowed only for paged operations.
    Page(usize),
    /// A value discovered in a successful upstream step.
    Reference(InputReference),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InputReference {
    /// Earlier step supplying the input.
    pub from_step: String,
    /// Must equal the upstream expectation's item path.
    pub items_path: Vec<String>,
    /// Bounded selection strategy.
    pub select: Selection,
    /// Literal field path resolving to a nonblank string.
    pub value_path: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Selection {
    /// First upstream valid item possessing the input; no request retry.
    FirstValid,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Limits {
    /// Per-step deadline in seconds.
    #[serde(default = "default_step_timeout")]
    pub step_timeout_seconds: u64,
    /// Total source deadline in seconds.
    #[serde(default = "default_source_timeout")]
    pub source_timeout_seconds: u64,
}

fn default_step_timeout() -> u64 {
    SOURCE_CHECK_STEP_TIMEOUT_SECONDS
}
fn default_source_timeout() -> u64 {
    SOURCE_CHECK_TIMEOUT_SECONDS
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            step_timeout_seconds: default_step_timeout(),
            source_timeout_seconds: default_source_timeout(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Prerequisite {
    /// Environment variable whose value must be present and nonblank.
    pub env: String,
    /// Human-readable reason, without secret values.
    pub reason: String,
}

impl SourceCheck {
    /// Loads and validates a contract and its production capabilities without requests.
    ///
    /// # Arguments
    /// * `path` - Contract mirroring its services-relative `source_config` path.
    /// * `services_root` - Production services directory, supplied by the application tests.
    ///
    /// # Errors
    /// Returns a contextual configuration error for malformed data, unsafe paths,
    /// contradictory capabilities or invalid dependencies. Missing files never
    /// fall back to inferred production assertions.
    pub fn load(path: &Path, services_root: &Path) -> Result<Self> {
        let load = || -> Result<Self> {
            let contract: Self = read_strict_yaml(File::open(path)?)?;
            contract.validate()?;
            ensure!(
                path.ends_with(Path::new(&contract.source_config)),
                "Contract path must mirror source_config below the fixture root"
            );
            ensure!(
                path.file_stem().and_then(|name| name.to_str()) == Some(contract.source.as_str()),
                "Contract filename must match source `{}`",
                contract.source
            );
            ensure!(
                path.extension().and_then(|value| value.to_str()) == Some("yaml"),
                "Source contracts must use .yaml"
            );
            let production_path = contract.configuration_path(services_root)?;
            let collection =
                ScraperQueryCollection::from_file_with(&production_path, serde_yaml::from_reader)
                    .with_context(|| {
                    format!(
                        "Unable to load production configuration {}",
                        production_path.display()
                    )
                })?;
            ensure!(
                collection.name() == contract.source,
                "Production id does not match contract source"
            );
            contract.validate_capabilities(&collection.query_names())?;
            Ok(contract)
        };
        load().with_context(|| format!("invalid_configuration: source contract {}", path.display()))
    }

    /// Checks data-only invariants before accessing the production configuration.
    ///
    /// # Errors
    /// Rejects invalid minima, operations, references, paths, capabilities or limits.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == SOURCE_CHECK_SCHEMA_VERSION,
            "Unsupported schema_version"
        );
        ensure!(!self.source.trim().is_empty(), "Blank source");
        ensure!(
            !self.steps.is_empty() && self.steps.len() <= SOURCE_CHECK_MAX_STEPS,
            "Invalid step count"
        );
        self.validate_config_path()?;
        ensure!(
            (1..=SOURCE_CHECK_MAX_STEP_TIMEOUT_SECONDS).contains(&self.limits.step_timeout_seconds)
                && (self.limits.step_timeout_seconds..=SOURCE_CHECK_MAX_TIMEOUT_SECONDS)
                    .contains(&self.limits.source_timeout_seconds),
            "Invalid execution timeouts"
        );
        for name in self
            .capabilities
            .expected
            .iter()
            .chain(&self.capabilities.not_expected)
        {
            serde_json::from_value::<Operation>(serde_json::Value::String(name.clone()))
                .with_context(|| format!("Unsupported capability `{name}`"))?;
        }
        let expected = unique_names(&self.capabilities.expected)?;
        let not_expected = unique_names(&self.capabilities.not_expected)?;
        ensure!(
            expected.is_disjoint(&not_expected),
            "Contradictory capabilities"
        );
        ensure!(
            expected.contains("service_stream_metadata"),
            "Metadata must be expected"
        );
        let mut previous: HashMap<&str, &Step> = HashMap::new();
        let mut covered = HashSet::new();
        for step in &self.steps {
            ensure!(
                !step.id.trim().is_empty() && !previous.contains_key(step.id.as_str()),
                "Blank or duplicate step id `{}`",
                step.id
            );
            ensure!(
                expected.contains(step.operation.name()),
                "Unexpected step operation `{}`",
                step.operation.name()
            );
            step.expect
                .validate()
                .with_context(|| format!("Step `{}` expectations", step.id))?;
            step.validate_inputs()
                .with_context(|| format!("Step `{}` inputs", step.id))?;
            for input in step.inputs.values() {
                if let Input::Reference(reference) = input {
                    let upstream =
                        previous
                            .get(reference.from_step.as_str())
                            .with_context(|| {
                                format!(
                                    "Step `{}` references a missing, forward or cyclic dependency",
                                    step.id
                                )
                            })?;
                    ensure!(
                        reference.items_path == upstream.expect.items_path,
                        "Step `{}` input path differs from upstream expectations",
                        step.id
                    );
                    validate_path(&reference.items_path, true)?;
                    validate_path(&reference.value_path, false)?;
                }
            }
            covered.insert(step.operation.name());
            previous.insert(&step.id, step);
        }
        ensure!(expected == covered, "Expected capabilities without steps");
        for step in &self.steps {
            if step.operation == Operation::LoadHome
                && step
                    .expect
                    .items_path
                    .first()
                    .is_some_and(|key| key == "categories")
            {
                ensure!(
                    self.has_content_dependent(&step.id),
                    "Home category navigation requires a dependent content step"
                );
            }
        }
        let mut prerequisites = HashSet::new();
        for prerequisite in &self.prerequisites {
            ensure!(
                !prerequisite.env.trim().is_empty()
                    && !prerequisite.reason.trim().is_empty()
                    && prerequisites.insert(&prerequisite.env),
                "Invalid or duplicate prerequisite"
            );
        }
        Ok(())
    }

    fn has_content_dependent(&self, id: &str) -> bool {
        self.steps.iter().any(|step| {
            step.inputs.values().any(
                |input| matches!(input, Input::Reference(reference) if reference.from_step == id),
            ) && (step.operation.is_content() || self.has_content_dependent(&step.id))
        })
    }

    fn validate_config_path(&self) -> Result<()> {
        let path = Path::new(&self.source_config);
        ensure!(
            !self.source_config.trim().is_empty()
                && !path.is_absolute()
                && path
                    .components()
                    .all(|part| matches!(part, Component::Normal(_)))
                && path.extension().and_then(|value| value.to_str()) == Some("yaml"),
            "source_config must be a services-relative YAML path without traversal"
        );
        Ok(())
    }

    /// Resolves the source path and rejects symlinks escaping the services root.
    ///
    /// # Arguments
    /// * `services_root` - Trusted production services directory.
    ///
    /// # Errors
    /// Rejects nonexistent configurations and paths outside the canonical root.
    pub fn configuration_path(&self, services_root: &Path) -> Result<PathBuf> {
        self.validate_config_path()?;
        let root = services_root.canonicalize()?;
        let path = root.join(&self.source_config).canonicalize()?;
        ensure!(
            path.starts_with(root) && path.is_file(),
            "source_config escapes services root or is not a file"
        );
        Ok(path)
    }

    /// Returns additional production operations without treating them as executed checks.
    ///
    /// # Arguments
    /// * `production` - Names advertised by the selected production collection.
    ///
    /// # Errors
    /// Rejects missing expected capabilities and present explicitly absent operations.
    pub fn validate_capabilities(&self, production: &[String]) -> Result<Vec<String>> {
        for expected in &self.capabilities.expected {
            ensure!(
                production.contains(expected),
                "Expected production operation `{expected}` is missing"
            );
        }
        for absent in &self.capabilities.not_expected {
            ensure!(
                !production.contains(absent),
                "Operation `{absent}` is declared not_expected but present"
            );
        }
        Ok(production
            .iter()
            .filter(|name| {
                !self.capabilities.expected.contains(name)
                    && !self.capabilities.not_expected.contains(name)
            })
            .cloned()
            .collect())
    }
}

impl Step {
    fn validate_inputs(&self) -> Result<()> {
        let required = match self.operation {
            Operation::ServiceStreamMetadata | Operation::LoadHome | Operation::ListLives => None,
            Operation::Search => Some("query"),
            _ => Some("query_url"),
        };
        if let Some(key) = required {
            ensure!(self.inputs.contains_key(key), "Missing input `{key}`");
        }
        for (key, input) in &self.inputs {
            if key == "page"
                && matches!(
                    self.operation,
                    Operation::GetCategory
                        | Operation::GetSection
                        | Operation::GetSeason
                        | Operation::GetRecommendations
                )
            {
                ensure!(
                    matches!(input, Input::Page(page) if *page > 0),
                    "Page must be a positive integer"
                );
            } else {
                ensure!(Some(key.as_str()) == required, "Unknown input `{key}`");
                match input {
                    Input::Literal(value) => {
                        ensure!(!value.trim().is_empty(), "Blank input `{key}`")
                    }
                    Input::Reference(reference) => {
                        ensure!(!reference.from_step.trim().is_empty(), "Blank from_step")
                    }
                    Input::Page(_) => anyhow::bail!("Input `{key}` must be a string or reference"),
                }
            }
        }
        Ok(())
    }
}

fn unique_names(names: &[String]) -> Result<HashSet<&str>> {
    let mut unique = HashSet::new();
    for name in names {
        ensure!(
            !name.trim().is_empty() && unique.insert(name.as_str()),
            "Blank or duplicate capability"
        );
    }
    Ok(unique)
}

#[cfg(test)]
#[path = "source_checks/schema.rs"]
mod schema_tests;
