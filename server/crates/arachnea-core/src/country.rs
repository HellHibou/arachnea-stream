//! Country-code normalization independent of routing and scraper policies.

/// Trims surrounding whitespace and uppercases ASCII letters without validation.
///
/// # Arguments
/// * `country` - Country hint, including permissive application-specific values.
pub fn normalize_country_code(country: &str) -> String {
    country.trim().to_ascii_uppercase()
}

/// Checks the two-ASCII-letter shape without consulting an ISO country registry.
///
/// # Arguments
/// * `country` - Code to check; surrounding whitespace is not removed here.
pub fn is_country_code(country: &str) -> bool {
    country.len() == 2 && country.bytes().all(|byte| byte.is_ascii_alphabetic())
}

/// Normalizes valid codes, discarding invalid inputs and retaining first-occurrence order.
///
/// # Arguments
/// * `countries` - Country hints; whitespace and ASCII case do not distinguish duplicates.
pub fn normalize_country_codes(
    countries: impl IntoIterator<Item = impl AsRef<str>>,
) -> Vec<String> {
    let mut normalized = Vec::new();
    for country in countries {
        let country = normalize_country_code(country.as_ref());
        if is_country_code(&country) && !normalized.contains(&country) {
            normalized.push(country);
        }
    }
    normalized
}

/// Normalizes valid codes into a sorted, deduplicated canonical representation.
///
/// # Arguments
/// * `countries` - Country hints; invalid inputs are discarded.
pub fn canonical_country_codes(
    countries: impl IntoIterator<Item = impl AsRef<str>>,
) -> Vec<String> {
    let mut normalized: Vec<_> = countries
        .into_iter()
        .map(|country| normalize_country_code(country.as_ref()))
        .filter(|country| is_country_code(country))
        .collect();
    normalized.sort_unstable();
    normalized.dedup();
    normalized
}

/// Normalizes codes in first-occurrence order, ignoring empty hints but rejecting invalid ones.
///
/// # Arguments
/// * `countries` - Country hints to trim, validate and uppercase.
///
/// # Errors
/// Returns the first nonempty invalid hint, trimmed and with its original case,
/// so the caller can provide a contextual diagnostic.
pub fn validate_country_codes(
    countries: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<Vec<String>, String> {
    let mut normalized = Vec::new();
    for country in countries {
        let country = country.as_ref().trim();
        if country.is_empty() {
            continue;
        }
        if !is_country_code(country) {
            return Err(country.to_string());
        }
        let country = normalize_country_code(country);
        if !normalized.contains(&country) {
            normalized.push(country);
        }
    }
    Ok(normalized)
}
