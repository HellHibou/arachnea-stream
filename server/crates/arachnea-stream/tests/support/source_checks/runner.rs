//! Bounded source execution, dependency resolution and per-step diagnostics.

use super::{
    execution,
    report::Report,
    source_checks::{Input, Operation, Selection, SourceCheck, Step},
    Prepared,
};
use anyhow::{ensure, Context, Result};
use arachnea_scrapyfy::scrapyfy::{
    scraper_cache::ScraperCacheConfig, scraper_manager::tests::assertions::AssertionOutcome,
};
use serde_json::Value;
use std::{
    collections::{BTreeSet, HashMap},
    time::Duration,
};
use tokio::time::{timeout, Instant};

/// Diagnostic overrides never become provider-specific defaults in Rust.
pub(super) struct Overrides {
    search: Option<String>,
    entry: Option<String>,
}

impl Overrides {
    /// Reads optional diagnostic overrides and rejects blank or non-Unicode values.
    pub fn load() -> Result<Self> {
        let read = |name: &str| -> Result<Option<String>> {
            match std::env::var(name) {
                Ok(value) => {
                    ensure!(
                        !value.trim().is_empty(),
                        "invalid_configuration: blank override {name}"
                    );
                    Ok(Some(value))
                }
                Err(std::env::VarError::NotPresent) => Ok(None),
                Err(_) => anyhow::bail!("invalid_configuration: non-Unicode override {name}"),
            }
        };
        Ok(Self {
            search: read("ARACHNEA_TEST_SEARCH_TERM")?,
            entry: read("ARACHNEA_TEST_ENTRY_URL")?,
        })
    }

    fn value(&self, step: &Step, key: &str) -> Option<(&'static str, &str)> {
        match (step.operation, key) {
            (Operation::Search, "query") => self
                .search
                .as_deref()
                .map(|value| ("ARACHNEA_TEST_SEARCH_TERM", value)),
            (Operation::GetEntry, "query_url") => self
                .entry
                .as_deref()
                .map(|value| ("ARACHNEA_TEST_ENTRY_URL", value)),
            _ => None,
        }
    }

    /// Selects operation steps and only their effective transitive dependencies.
    ///
    /// # Arguments
    /// * `contract` - Validated source-check graph.
    /// * `operation` - Optional operation filter; absence selects every step.
    ///
    /// # Errors
    /// Rejects uncovered requested operations and irrelevant diagnostic overrides.
    pub fn select_steps(
        &self,
        contract: &SourceCheck,
        operation: Option<Operation>,
    ) -> Result<BTreeSet<usize>> {
        let mut selected = contract
            .steps
            .iter()
            .enumerate()
            .filter(|(_, step)| operation.is_none_or(|operation| step.operation == operation))
            .map(|(index, _)| index)
            .collect::<BTreeSet<_>>();
        if let Some(operation) = operation {
            ensure!(
                !selected.is_empty()
                    || contract
                        .capabilities
                        .not_expected
                        .iter()
                        .any(|name| name == operation.name()),
                "coverage_missing: requested operation `{}` has no scenario",
                operation.name()
            );
        }
        // Backward-only references were already checked by the strict loader.
        for index in (0..contract.steps.len()).rev() {
            if !selected.contains(&index) {
                continue;
            }
            let step = &contract.steps[index];
            for (key, input) in &step.inputs {
                if self.value(step, key).is_some() {
                    continue;
                }
                if let Input::Reference(reference) = input {
                    let upstream = contract
                        .steps
                        .iter()
                        .position(|step| step.id == reference.from_step)
                        .context("invalid_configuration: missing dependency")?;
                    selected.insert(upstream);
                }
            }
        }
        for (present, operation, name) in [
            (
                self.search.is_some(),
                Operation::Search,
                "ARACHNEA_TEST_SEARCH_TERM",
            ),
            (
                self.entry.is_some(),
                Operation::GetEntry,
                "ARACHNEA_TEST_ENTRY_URL",
            ),
        ] {
            ensure!(
                !present
                    || selected
                        .iter()
                        .any(|index| contract.steps[*index].operation == operation),
                "invalid_configuration: irrelevant override {name}"
            );
        }
        Ok(selected)
    }

    fn resolve(
        &self,
        step: &Step,
        contract: &SourceCheck,
        passed: &HashMap<String, Vec<Value>>,
    ) -> Result<HashMap<String, Value>> {
        let mut inputs = HashMap::new();
        for (key, input) in &step.inputs {
            let value = if let Some((name, value)) = self.value(step, key) {
                println!(
                    "source={} step={} diagnostic_override={name} reference={}",
                    contract.source,
                    step.id,
                    execution::reference(value)
                );
                Value::String(value.to_string())
            } else {
                match input {
                    Input::Literal(value) => Value::String(value.clone()),
                    Input::Page(value) => Value::from(*value as u64),
                    Input::Reference(reference) => {
                        let roots = passed.get(&reference.from_step).with_context(|| {
                            format!("Upstream step `{}` did not pass", reference.from_step)
                        })?;
                        let upstream = contract
                            .steps
                            .iter()
                            .find(|step| step.id == reference.from_step)
                            .context("Missing validated upstream step")?;
                        let value = match reference.select {
                            Selection::FirstValid => upstream
                                .expect
                                .first_valid_string(roots, &reference.value_path)?,
                        };
                        Value::String(value)
                    }
                }
            };
            inputs.insert(key.clone(), value);
        }
        Ok(inputs)
    }
}

/// Executes selected steps without runner retries and reports blocked descendants separately.
///
/// # Arguments
/// * `prepared` - Isolated facade, contract and owned temporary directory.
/// * `selected` - Already-validated step selection and dependencies.
/// * `operation` - Requested filter, or a full source check.
/// * `overrides` - Explicit diagnostic input replacements.
/// * `table` - Accumulator for the final human-readable result table.
///
/// # Errors
/// Fails if any expected step fails/is blocked, a prerequisite is absent, or a
/// full run has no successful content step. Nonapplicable filters are explicit.
pub(super) async fn run_source(
    prepared: &mut Prepared,
    selected: &BTreeSet<usize>,
    operation: Option<Operation>,
    overrides: &Overrides,
    table: &mut Report,
) -> Result<()> {
    let source = &prepared.contract.source;
    println!("source={source} selected_steps={} diagnostic={} partial_coverage={} cache=fresh_per_step uncovered={:?}",
        selected.len(), overrides.search.is_some() || overrides.entry.is_some(),
        operation.is_some() || !prepared.uncovered.is_empty(), prepared.uncovered);
    for name in &prepared.uncovered {
        println!("source={source} operation={name} outcome=coverage_missing");
        table.record(
            source,
            name,
            "coverage_missing",
            "test not implemented for this production capability",
        );
    }
    println!(
        "source={source} not_expected={:?} excluded_from_results=true",
        prepared.contract.capabilities.not_expected
    );
    let excluded = prepared
        .contract
        .steps
        .iter()
        .enumerate()
        .filter(|(index, _)| !selected.contains(index))
        .map(|(_, step)| step.id.as_str())
        .collect::<Vec<_>>();
    println!("source={source} excluded_steps={excluded:?}");
    if selected.is_empty() {
        println!("source={source} outcome=not_applicable executed=0");
        return Ok(());
    }
    let missing = prepared
        .contract
        .prerequisites
        .iter()
        .filter(|prerequisite| {
            std::env::var(&prerequisite.env).map_or(true, |value| value.trim().is_empty())
        })
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        for prerequisite in &missing {
            let reason = execution::sanitize(&prerequisite.reason);
            println!(
                "source={source} outcome=prerequisite_missing env={} reason={reason}",
                prerequisite.env
            );
            table.record(
                source,
                "prerequisites",
                "prerequisite_missing",
                format!("env={} reason={reason}", prerequisite.env),
            );
        }
        anyhow::bail!("prerequisite_missing: required environment values are absent");
    }
    let start = Instant::now();
    let deadline = start + Duration::from_secs(prepared.contract.limits.source_timeout_seconds);
    let mut passed = HashMap::new();
    let mut failures = 0;
    let mut failure_reasons = Vec::new();
    let mut blocked = 0;
    let mut executed = 0;
    let mut content_passed = 0;
    for index in selected {
        let step = &prepared.contract.steps[*index];
        let step_start = Instant::now();
        println!("source={source} step={} operation={} planned_path={:?} min_count={} min_valid_count={}",
            step.id, step.operation.name(), step.expect.items_path, step.expect.min_count, step.expect.min_valid_count);
        let inputs = match overrides.resolve(step, &prepared.contract, &passed) {
            Ok(inputs) if step_start < deadline => inputs,
            result => {
                blocked += 1;
                let reason = if step_start >= deadline {
                    "Source deadline exhausted".into()
                } else {
                    format!("{:#}", result.unwrap_err())
                };
                let reason = execution::sanitize(&reason);
                failure_reasons.push(format!(
                    "step={} operation={} blocked: {reason}",
                    step.id,
                    step.operation.name()
                ));
                println!(
                    "source={source} step={} operation={} outcome=blocked reason={reason}",
                    step.id,
                    step.operation.name()
                );
                table.record(source, step.operation.name(), "blocked", reason);
                continue;
            }
        };
        let references = inputs
            .iter()
            .map(|(key, value)| {
                (
                    key,
                    value
                        .as_str()
                        .map(execution::reference)
                        .unwrap_or_else(|| value.to_string()),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        println!(
            "source={source} step={} operation={} inputs={references:?}",
            step.id,
            step.operation.name()
        );
        let cache_dir = prepared.directory.0.join(format!("step-{index}"));
        std::fs::create_dir(&cache_dir).context("Unable to create fresh step cache")?;
        ensure!(
            std::fs::read_dir(&cache_dir)?.next().is_none(),
            "Fresh step cache directory is not empty"
        );
        let cache_id = arachnea_core::crypt::hash62_64(cache_dir.to_string_lossy().as_bytes());
        println!("source={source} step={} cache_id={cache_id} cache_directory_new=true cache_directory_empty=true", step.id);
        let cache_config = ScraperCacheConfig {
            cache_dir,
            ..Default::default()
        };
        prepared
            .scraper
            .scraper_agregator
            .set_cache_config(cache_config.clone());
        let cache = prepared.scraper.scraper_agregator.server_cache();
        let cached_operation = matches!(
            step.operation,
            Operation::LoadHome | Operation::GetCategory | Operation::GetSection
        );
        let budget = Duration::from_secs(prepared.contract.limits.step_timeout_seconds)
            .min(deadline.saturating_duration_since(Instant::now()));
        executed += 1;
        let result = timeout(budget, async {
            if cached_operation {
                let handle = cache.handle().await.map_err(execution::Failure::from)?;
                let usage = handle.memory().usage();
                println!(
                    "source={source} step={} cache_id={cache_id} memory_usage_before={usage}",
                    step.id
                );
                if usage != 0 {
                    return Err(execution::Failure::from(anyhow::anyhow!(
                        "Fresh step cache contains memory entries"
                    )));
                }
            }
            execution::execute(
                &prepared.scraper,
                &prepared.contract,
                step.operation,
                &inputs,
            )
            .await
        })
        .await;
        // Close the hybrid cache before dropping handles/removing the owned directory.
        let cleanup = if cached_operation {
            timeout(
                Duration::from_secs(crate::SOURCE_CHECK_CLEANUP_TIMEOUT_SECONDS),
                async {
                    let handle = cache.handle().await?;
                    println!(
                        "source={source} step={} cache_id={cache_id} memory_usage_after={}",
                        step.id,
                        handle.memory().usage()
                    );
                    handle.close().await?;
                    Ok::<_, anyhow::Error>(())
                },
            )
            .await
            .map_err(anyhow::Error::from)
            .and_then(|result| result)
        } else {
            Ok(())
        };
        prepared
            .scraper
            .scraper_agregator
            .set_cache_config(cache_config);
        drop(cache);
        let elapsed = step_start.elapsed().as_millis();
        if let Err(error) = cleanup {
            failures += 1;
            let cause = format!(
                "cache_cleanup: {}",
                execution::sanitize(&format!("{error:#}"))
            );
            failure_reasons.push(format!("step={} {cause}", step.id));
            println!("source={source} step={} operation={} outcome=contract_failed duration_ms={elapsed} cause={cause}",
                step.id, step.operation.name());
            table.record(source, step.operation.name(), "contract_failed", cause);
            continue;
        }
        match result {
            Err(_) => {
                failures += 1;
                failure_reasons.push(format!(
                    "step={} operation={} timeout",
                    step.id,
                    step.operation.name()
                ));
                println!("source={source} step={} operation={} outcome=retrieval_failed duration_ms={elapsed} cause=timeout", step.id, step.operation.name());
                table.record(
                    source,
                    step.operation.name(),
                    "retrieval_failed",
                    format!("timeout after {elapsed} ms"),
                );
            }
            Ok(Err(failure)) => {
                failures += 1;
                let mut cause = format!("{:#}", failure.error);
                for value in inputs.values().filter_map(Value::as_str) {
                    cause = cause.replace(value, &execution::reference(value));
                }
                let cause = execution::sanitize(&cause);
                failure_reasons.push(format!(
                    "step={} operation={} {}: {cause}",
                    step.id,
                    step.operation.name(),
                    failure.outcome
                ));
                println!("source={source} step={} operation={} outcome={} duration_ms={elapsed} cause={cause}",
                    step.id, step.operation.name(), failure.outcome);
                table.record(source, step.operation.name(), failure.outcome, cause);
            }
            Ok(Ok(roots)) => {
                let report = step.expect.evaluate(&roots);
                println!("source={source} step={} operation={} outcome={} duration_ms={elapsed} path={:?} min_count={} min_valid_count={} total={} valid={} invalid={} shape_errors={:?} invalid_fields={:?} invalid_types={:?}",
                    step.id, step.operation.name(), report.outcome.as_str(), step.expect.items_path,
                    step.expect.min_count, step.expect.min_valid_count, report.total, report.valid, report.invalid,
                    report.shape_errors, report.invalid_fields, report.invalid_types);
                if report.outcome == AssertionOutcome::Passed {
                    table.record(source, step.operation.name(), "passed", "");
                    if step.operation.is_content() {
                        content_passed += 1;
                    }
                    passed.insert(step.id.clone(), roots);
                } else {
                    failures += 1;
                    let message = format!("path={:?} total={} valid={} invalid={} invalid_fields={:?} invalid_types={:?} shape_errors={:?}",
                        step.expect.items_path, report.total, report.valid, report.invalid,
                        report.invalid_fields, report.invalid_types, report.shape_errors);
                    failure_reasons.push(format!(
                        "step={} operation={} {}: {message}",
                        step.id,
                        step.operation.name(),
                        report.outcome.as_str()
                    ));
                    table.record(
                        source,
                        step.operation.name(),
                        report.outcome.as_str(),
                        message,
                    );
                }
            }
        }
    }
    println!("source={source} summary executed={executed} passed={} primary_failures={failures} blocked={blocked} content_passed={content_passed} duration_ms={}", passed.len(), start.elapsed().as_millis());
    ensure!(
        failures == 0 && blocked == 0,
        "{failures} primary failures, {blocked} blocked steps: {}",
        failure_reasons.join("; ")
    );
    ensure!(
        executed > 0 && (operation.is_some() || content_passed > 0),
        "No successful content check executed"
    );
    Ok(())
}
