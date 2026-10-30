//! Existing Stream scenarios driven by independent source-check contracts.

#[path = "source_checks/execution.rs"]
mod execution;
#[path = "source_checks/report.rs"]
mod report;
#[path = "source_checks/runner.rs"]
mod runner;
#[path = "source_checks.rs"]
mod source_checks;

use super::StreamScraper;
use anyhow::{ensure, Context, Result};
use arachnea_core::persistence::FileCredentialsStore;
use arachnea_scrapyfy::scrapyfy::{
    scraper_agregator::{resolve_manifest_sources, ScraperAggregatorSourceEntry},
    scraper_query_collection::ScraperQueryCollection,
};
use source_checks::{Operation, SourceCheck};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

/// Executes the selected source, or all production YAML when selection is absent or `*`.
pub(super) fn test_all_services() -> Result<()> {
    run(selected_paths()?, None)
}

/// Preserves the existing full-scenario entry point with the same selection rules.
pub(super) fn test_service() -> Result<()> {
    test_all_services()
}

/// Executes metadata locally for the selection, or all checked-in contracts.
pub(super) fn service_stream_metadata() -> Result<()> {
    let paths = if let Some(source) = query_source()? {
        vec![resolve_source(&source)?]
    } else {
        let mut paths = Vec::new();
        collect_yaml_paths(&fixtures_root(), &mut paths)?;
        paths.sort();
        // Derive the production path without loading contracts outside per-source isolation.
        paths
            .into_iter()
            .map(|path| {
                let root = fixtures_root();
                let relative = path.strip_prefix(&root)?;
                Ok(services_root().join(relative))
            })
            .collect::<Result<Vec<_>>>()?
    };
    run(paths, Some(Operation::ServiceStreamMetadata))
}

pub(super) fn load_home() -> Result<()> {
    run(selected_paths()?, Some(Operation::LoadHome))
}

pub(super) fn search() -> Result<()> {
    run(selected_paths()?, Some(Operation::Search))
}

pub(super) fn get_entry() -> Result<()> {
    run(selected_paths()?, Some(Operation::GetEntry))
}

fn services_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../services")
}

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(crate::SOURCE_CHECK_FIXTURES_DIRECTORY)
}

/// Collects production or fixture YAML files without following symbolic links.
///
/// # Arguments
/// * `directory` - Source or fixture directory to walk.
/// * `paths` - Output list, sorted by the caller before loading.
///
/// # Errors
/// Rejects unreadable directories and symbolic links in the scanned tree.
fn collect_yaml_paths(directory: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        ensure!(
            !file_type.is_symlink(),
            "invalid_configuration: symlinks in service-check directories are not supported"
        );
        let path = entry.path();
        if file_type.is_dir() {
            collect_yaml_paths(&path, paths)?;
        } else if file_type.is_file()
            && path.extension().and_then(|value| value.to_str()) == Some("yaml")
        {
            paths.push(path);
        }
    }
    Ok(())
}

fn manifest_sources(root: &Path) -> Result<Vec<ScraperAggregatorSourceEntry>> {
    resolve_manifest_sources(
        root.join(crate::STREAM_SERVICE_GROUP_NAME)
            .join("services.json"),
    )
    .context("invalid_configuration: unable to resolve Stream manifest")
}

/// Reads an optional selector; only an absent variable or exact `*` selects all.
fn query_source() -> Result<Option<String>> {
    match std::env::var("ARACHNEA_TEST_QUERY_SOURCE") {
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(_) => anyhow::bail!("invalid_configuration: non-Unicode ARACHNEA_TEST_QUERY_SOURCE"),
        Ok(value) if value == "*" => Ok(None),
        Ok(value) => {
            ensure!(
                !value.trim().is_empty(),
                "invalid_configuration: blank ARACHNEA_TEST_QUERY_SOURCE"
            );
            Ok(Some(value))
        }
    }
}

/// Selects one configuration or discovers every production Stream YAML.
fn selected_paths() -> Result<Vec<PathBuf>> {
    if let Some(source) = query_source()? {
        let path = resolve_source(&source)?;
        println!(
            "selection=single_source configuration_path={}",
            path.display()
        );
        return Ok(vec![path]);
    }
    let mut paths = Vec::new();
    collect_yaml_paths(
        &services_root().join(crate::STREAM_SERVICE_GROUP_NAME),
        &mut paths,
    )?;
    paths.sort();
    println!(
        "selection=all_service_yaml discovered={} includes_disabled=true",
        paths.len()
    );
    Ok(paths)
}

/// Resolves a unique production ID or a legacy services-relative path.
///
/// # Arguments
/// * `value` - Nonblank source ID or services-relative YAML path, excluding `*`.
///
/// # Errors
/// Rejects unsafe paths, unreadable YAML and unknown or ambiguous IDs.
fn resolve_source(value: &str) -> Result<PathBuf> {
    let relative = Path::new(value);
    ensure!(
        !value.trim().is_empty() && !relative.is_absolute()
            && relative.components().all(|part| matches!(part, Component::Normal(_))),
        "invalid_configuration: source selection must be an ID or a services-relative path without traversal"
    );
    let group_root = services_root()
        .join(crate::STREAM_SERVICE_GROUP_NAME)
        .canonicalize()?;
    if relative.components().count() == 1 {
        let mut paths = Vec::new();
        collect_yaml_paths(&group_root, &mut paths)?;
        paths.sort();
        let mut matches = Vec::new();
        for path in paths {
            let collection = ScraperQueryCollection::from_file_with(&path, serde_yaml::from_reader)
                .with_context(|| {
                    format!("invalid_configuration: unable to read {}", path.display())
                })?;
            if collection.name() == value {
                matches.push(path);
            }
        }
        ensure!(matches.len() == 1, "invalid_configuration: service ID `{value}` matched {} production YAML files; expected exactly one", matches.len());
        return Ok(matches.remove(0));
    }
    let path = services_root()
        .join(relative)
        .canonicalize()
        .context("invalid_configuration: selected source path is missing")?;
    ensure!(
        path.starts_with(group_root),
        "invalid_configuration: source selection escapes the Stream services directory"
    );
    Ok(path)
}

fn run(paths: Vec<PathBuf>, operation: Option<Operation>) -> Result<()> {
    crate::test_support::configure_test_application_data_dir();
    ensure!(!paths.is_empty(), "coverage_missing: no sources selected");
    let root = services_root();
    let manifest = manifest_sources(&root)?;
    let overrides = runner::Overrides::load()?;
    let selected_count = paths.len();
    let mut prepared = Vec::new();
    let mut results = Vec::new();
    let mut table = report::Report::default();
    let mut partial_coverage = operation.is_some();
    // Validate every source before requests, but keep failures local to that source.
    for path in paths {
        let preparation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Prepared::load(&path, &root, &manifest).and_then(|prepared| {
                let selected = overrides.select_steps(&prepared.contract, operation)?;
                Ok((prepared, selected))
            })
        }))
        .unwrap_or_else(|panic| Err(panic_error(panic)));
        match preparation {
            Ok(value) => prepared.push(value),
            Err(error) => {
                let missing = error.downcast_ref::<MissingSourceContract>();
                let source = missing
                    .map(|missing| missing.source.clone())
                    .unwrap_or_else(|| path.display().to_string());
                let outcome = if missing.is_some() {
                    "not_tested"
                } else {
                    "failed"
                };
                let reason = execution::sanitize(&format!("{error:#}"));
                println!(
                    "source={source} outcome={outcome} configuration_path={} reason={reason}",
                    path.display()
                );
                table.record(&source, "preflight", outcome, &reason);
                results.push(SourceResult {
                    source,
                    outcome,
                    reason,
                });
                partial_coverage = true;
            }
        }
    }
    println!(
        "preflight_summary selected={selected_count} ready={} not_tested={} failed={}",
        prepared.len(),
        results
            .iter()
            .filter(|result| result.outcome == "not_tested")
            .count(),
        results
            .iter()
            .filter(|result| result.outcome == "failed")
            .count()
    );
    let mut attempted = 0;
    if !prepared.is_empty() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        for (mut prepared, selected) in prepared {
            let source = prepared.contract.source.clone();
            partial_coverage |= !prepared.uncovered.is_empty();
            if !selected.is_empty() {
                attempted += 1;
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                runtime.block_on(runner::run_source(
                    &mut prepared,
                    &selected,
                    operation,
                    &overrides,
                    &mut table,
                ))
            }));
            let (outcome, reason) = match result {
                Ok(Ok(())) if selected.is_empty() => (
                    "not_applicable",
                    "Requested operation is explicitly not expected".to_string(),
                ),
                Ok(Ok(())) => ("passed", "Selected checks passed".to_string()),
                Ok(Err(error)) => ("failed", execution::sanitize(&format!("{error:#}"))),
                Err(panic) => (
                    "failed",
                    execution::sanitize(&format!("{:#}", panic_error(panic))),
                ),
            };
            if outcome == "failed" && !table.has_error(&source) {
                table.record(&source, "run", outcome, &reason);
            }
            results.push(SourceResult {
                source,
                outcome,
                reason,
            });
        }
    }
    results.sort_by(|left, right| left.source.cmp(&right.source));
    let count = |outcome| {
        results
            .iter()
            .filter(|result| result.outcome == outcome)
            .count()
    };
    let passed = count("passed");
    let failed = count("failed");
    let not_tested = count("not_tested");
    let not_applicable = count("not_applicable");
    println!("source_check_summary selected={selected_count} attempted={attempted} passed={passed} failed={failed} not_tested={not_tested} not_applicable={not_applicable} partial_coverage={partial_coverage}");
    for result in &results {
        println!(
            "source_check_result source={} outcome={} reason={}",
            result.source, result.outcome, result.reason
        );
    }
    table.print();
    ensure!(failed == 0 && not_tested == 0,
        "Source checks finished: {passed} passed, {failed} failed, {not_tested} not tested, {not_applicable} not applicable; see source_check_summary");
    Ok(())
}

/// Converts an unwound source panic into a reportable error without reusing its state.
fn panic_error(panic: Box<dyn std::any::Any + Send>) -> anyhow::Error {
    let message = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .unwrap_or("Non-string panic payload");
    anyhow::anyhow!("Service check panicked: {message}")
}

/// One source's final status; missing contracts are distinct from executed failures.
struct SourceResult {
    source: String,
    outcome: &'static str,
    reason: String,
}

/// Typed coverage error, so unavailable contracts are not classified by error-text matching.
#[derive(Debug)]
struct MissingSourceContract {
    source: String,
}

impl std::fmt::Display for MissingSourceContract {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "coverage_missing: no source contract for `{}`; service is not tested",
            self.source
        )
    }
}

impl std::error::Error for MissingSourceContract {}

/// Owns the isolated facade before its temporary files, so handles drop first.
struct Prepared {
    scraper: StreamScraper,
    contract: SourceCheck,
    uncovered: Vec<String>,
    directory: TemporaryDirectory,
}

impl Prepared {
    fn load(path: &Path, root: &Path, manifest: &[ScraperAggregatorSourceEntry]) -> Result<Self> {
        let path = path
            .canonicalize()
            .context("invalid_configuration: selected configuration is missing")?;
        let collection = ScraperQueryCollection::from_file_with(&path, serde_yaml::from_reader)
            .context("invalid_configuration: unable to read selected source")?;
        let canonical_root = root.canonicalize()?;
        let relative = path
            .strip_prefix(&canonical_root)
            .context("invalid_configuration: selected source escapes services root")?;
        let fixture = fixtures_root().join(relative);
        if !fixture.try_exists()? {
            return Err(MissingSourceContract {
                source: collection.name().to_string(),
            }
            .into());
        }
        ensure!(
            fixture.is_file(),
            "invalid_configuration: source contract is not a file"
        );
        let contract = SourceCheck::load(&fixture, root)?;
        ensure!(
            contract.configuration_path(root)? == path,
            "invalid_configuration: selected path differs from source contract"
        );
        let uncovered = contract.validate_capabilities(&collection.query_names())?;
        let mut matches = manifest.iter().filter(|entry| {
            root.join(crate::STREAM_SERVICE_GROUP_NAME)
                .join(&entry.path)
                .canonicalize()
                .is_ok_and(|candidate| candidate == path)
        });
        let mut entry = matches
            .next()
            .cloned()
            .unwrap_or_else(|| ScraperAggregatorSourceEntry {
                path: path.to_string_lossy().into_owned(),
                enabled: true,
                parameters: Vec::new(),
            });
        ensure!(
            matches.next().is_none(),
            "invalid_configuration: duplicate selected source in manifest"
        );
        // A one-source manifest retains effective overrides and explicitly enables disabled selections.
        entry.path = path.to_string_lossy().into_owned();
        entry.enabled = true;
        let directory = TemporaryDirectory::new()?;
        let manifest_path = directory.0.join("services.json");
        serde_json::to_writer(fs::File::create(&manifest_path)?, &vec![entry.clone()])?;
        let mut scraper = StreamScraper::new(
            FileCredentialsStore::new(directory.0.join("credentials.json")).as_arc(),
        );
        scraper
            .scraper_agregator
            .add_query_collection_from_config_json(
                crate::STREAM_SERVICE_GROUP_NAME,
                &manifest_path,
            )?;
        let loaded = scraper
            .scraper_agregator
            .query_services()
            .filter(|group| group.name == crate::STREAM_SERVICE_GROUP_NAME)
            .flat_map(|group| &group.services)
            .collect::<Vec<_>>();
        ensure!(
            loaded.len() == 1 && loaded[0].name() == contract.source,
            "invalid_configuration: facade must contain exactly the selected source"
        );
        for expected in &entry.parameters {
            let actual = loaded[0]
                .parameters()
                .iter()
                .find(|parameter| parameter.name == expected.name)
                .context("Manifest override disappeared from the isolated source")?;
            ensure!(
                serde_json::to_value(actual)? == serde_json::to_value(expected)?,
                "Manifest override changed while preparing the isolated source"
            );
        }
        Ok(Self {
            scraper,
            contract,
            uncovered,
            directory,
        })
    }
}

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn new() -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "arachnea-source-check-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        fs::create_dir(&path).context("Unable to create isolated source-check directory")?;
        Ok(Self(path))
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("Source-check temporary cleanup failed: {error}");
        }
    }
}
