//! Recursive source manifest expansion with cycle detection.

use super::*;

/// Resolves every source declared by a services manifest into a flattened list,
/// following recursive imports in depth-first declaration order.
///
/// # Arguments
///
/// * `config_path` - Path to the top-level JSON manifest.
///
/// # Returns
///
/// The flattened [`ScraperAggregatorSourceEntry`] list. Each entry keeps its
/// `enabled` state and parameters, and its `path` is resolved relative to the
/// directory containing `config_path`.
///
/// # Errors
///
/// Returns an error when the manifest itself, an imported JSON manifest, or
/// their content is missing or invalid. Error messages name the parent manifest
/// and the offending entry or import path.
pub fn resolve_manifest_sources(
    config_path: impl AsRef<Path>,
) -> Result<Vec<ScraperAggregatorSourceEntry>> {
    let config_path = config_path.as_ref();
    let mut resolved: Vec<ScraperAggregatorSourceEntry> = Vec::new();
    let mut visited: Vec<String> = Vec::new();
    let mut import_chain: Vec<String> = Vec::new();
    import_chain.push(format!("{}", config_path.display()));

    let empty_rel_dir = PathBuf::new();
    resolve_manifest_sources_recursive(
        config_path,
        &empty_rel_dir,
        &mut visited,
        &mut import_chain,
        &mut resolved,
    )?;

    Ok(resolved)
}

/// Expands manifest imports recursively, skipping repeated identities and cycles with a warning.
///
/// # Arguments
///
/// * `config_path` - Path of the JSON manifest currently being expanded.
/// * `manifest_rel_dir` - Directory against which declared source paths are normalized.
/// * `visited` - Manifest identities already expanded, used to skip duplicate imports and cycles.
/// * `import_chain` - Current import stack retained for diagnostic messages.
/// * `resolved` - Output source entries accumulated in manifest traversal order.
///
/// # Errors
///
/// Returns errors when a manifest cannot be opened or decoded, or a source/import path cannot be resolved. Duplicate and cyclic imports are skipped with a warning.
pub(super) fn resolve_manifest_sources_recursive(
    config_path: &Path,
    manifest_rel_dir: &Path,
    visited: &mut Vec<String>,
    import_chain: &mut Vec<String>,
    resolved: &mut Vec<ScraperAggregatorSourceEntry>,
) -> Result<()> {
    // A manifest already imported by another branch, or a cycle back onto the
    // current import chain, is skipped with a warning so resolution terminates.
    let manifest_identity = format!("{}", config_path.display());
    if visited.contains(&manifest_identity) {
        tracing::warn!(target: "arachnea_scrapyfy::scrapyfy::scraper_agregator",
            config_path = %config_path.display(),
            import_chain = %import_chain.join(" -> "),
            "skipping already-imported or cyclic services manifest"
        );
        return Ok(());
    }
    visited.push(manifest_identity);

    let manifest_dir = config_path.parent().unwrap_or(Path::new(""));
    let config_file = fs::File::open(config_path).map_err(|error| {
        anyhow::anyhow!(
            "Failed to open manifest file: {}: {error}",
            config_path.display()
        )
    })?;
    let raw_entries: Vec<serde_json::Value> = serde_json::from_reader(BufReader::new(config_file))
        .map_err(|error| {
            anyhow::anyhow!(
                "Failed to parse manifest file: {}: {error}",
                config_path.display()
            )
        })?;

    for (entry_index, raw) in raw_entries.into_iter().enumerate() {
        let object = match &raw {
            serde_json::Value::Object(entries) => entries,
            _ => anyhow::bail!(
                "Manifest entry must be a JSON object: {}: entry {}",
                config_path.display(),
                entry_index
            ),
        };

        // Reject entries declaring both or neither of `path`/`import` explicitly,
        // since `untagged` would otherwise silently pick the first variant.
        let has_path = object.get("path").is_some();
        let has_import = object.get("import").is_some();
        if has_path == has_import {
            anyhow::bail!(
                "Manifest entry must declare exactly one of `path` or `import`: {}: entry {}",
                config_path.display(),
                entry_index
            );
        }

        let entry: ScraperAggregatorConfigEntry = serde_json::from_value(raw).map_err(|error| {
            anyhow::anyhow!(
                "Failed to parse manifest entry: {}: entry {}: {error}",
                config_path.display(),
                entry_index
            )
        })?;

        match entry {
            ScraperAggregatorConfigEntry::Source(source) => {
                let resolved_path = manifest_rel_dir.join(Path::new(&source.path));
                resolved.push(ScraperAggregatorSourceEntry {
                    path: format!("{}", resolved_path.display()),
                    enabled: source.enabled,
                    parameters: source.parameters,
                });
            }
            ScraperAggregatorConfigEntry::Import(import) => {
                let child_rel_dir = manifest_rel_dir
                    .join(Path::new(&import.import).parent().unwrap_or(Path::new("")));
                let child_path = manifest_dir.join(&import.import);
                import_chain.push(import.import.clone());
                let imported = resolve_manifest_sources_recursive(
                    &child_path,
                    &child_rel_dir,
                    visited,
                    import_chain,
                    resolved,
                );
                import_chain.pop();
                if let Err(error) = imported {
                    return Err(anyhow::anyhow!(
                        "Failed to import `{}` from {}: {error:#}",
                        import.import,
                        config_path.display()
                    ));
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/scrapyfy/scraper_agregator/manifest.rs"]
mod tests;
