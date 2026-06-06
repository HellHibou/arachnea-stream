use anyhow::{bail, Context, Result};
use chrono::{DateTime, Duration, Local, NaiveDateTime};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping as YamlMapping, Value as YamlValue};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use url::Url;
use quick_html2md;

use crate::scrapyfy::query_helpers;

static REGEX_CACHE: OnceLock<Mutex<HashMap<String, Regex>>> = OnceLock::new();

static PLACEHOLDER_REGEX: OnceLock<Regex> = OnceLock::new();

/// Supported input formats accepted by `get_date`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GetDateSource {
    /// Matches dates like `2026-03-25` or `2026-03-25T20:15:00+01:00`.
    YyyyMmDd,
    /// Matches dates like `25/03/2026` or `25.03.2026`.
    DdMmYyyy,
    /// Matches dates like `25/03/26` or `25.03.26`.
    DdMmYy,
    /// Matches dates like `mar. 25 mars 2026`, using the configured `months`.
    DdMonthYyyy,
    /// Matches a day count such as `3` and returns today's date plus that many days.
    DaysFromToday,
    /// Matches date-times like `2026-03-25T20:15:00+01:00` or `2026-03-25 20:15:00`.
    YyyyMmDdHhMmSs,
}

/// One or many input formats accepted by `get_date`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GetDateSources {
    One(GetDateSource),
    Many(Vec<GetDateSource>),
}

impl GetDateSources {
    fn find_map<T, F>(&self, mut callback: F) -> Option<T>
    where
        F: FnMut(GetDateSource) -> Option<T>,
    {
        match self {
            Self::One(source) => callback(*source),
            Self::Many(sources) => sources.iter().copied().find_map(&mut callback),
        }
    }

    fn contains_named_month_source(&self) -> bool {
        self.find_map(|source| (source == GetDateSource::DdMonthYyyy).then_some(()))
            .is_some()
    }

    fn is_empty(&self) -> bool {
        matches!(self, Self::Many(sources) if sources.is_empty())
    }
}

/// Ordered extraction steps applied during a scraper extraction pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScraperAction {
    /// Reads the concatenated text content from the selected element and appends it
    /// to the current value list.
    GetText,
    /// Converts HTML content of the selected element to plain text using `quick_html2md`.
    HtmlToText,

    /// Reads one HTML attribute from the selected element and appends its value to
    /// the current value list.
    ///
    /// # Fields
    ///
    /// * `argument` - Attribute name to extract, such as `href`, `src`, or `content`.
    GetAttribut {
        /// Attribute name to read from the selected element.
        argument: String,
    },

    /// Splits every current value using the provided separator and replaces the
    /// current value list with the produced fragments.
    ///
    /// # Fields
    ///
    /// * `argument` - Separator string used with `str::split`.
    Split {
        /// Separator used to split previously extracted values.
        argument: String,
    },

    /// Rewrites each current value using a lookup table and keeps the original value
    /// when no mapping entry exists, unless a default scalar is provided.
    /// Mapping keys and values accept YAML scalars, including `null`, `true`, `false`,
    /// numbers, and strings. Mapped `null` values are dropped from the output list.
    ///
    /// # Fields
    ///
    /// * `argument` - Mapping table from raw extracted values to normalized values.
    ///   A `null` key matches an explicit `null` input, and a `null` mapped value
    ///   removes the current entry from the output list.
    /// * `default` - Optional fallback scalar used when no mapping entry exists.
    ///   When it is a string, the `{}` placeholder is replaced with the original value.
    Map {
        /// Lookup table used to normalize extracted values.
        argument: YamlMapping,
        /// Optional fallback scalar applied when the input value is not mapped.
        #[serde(default)]
        default: Option<YamlValue>,
    },

    /// Applies a regular expression to every current value and replaces the current
    /// value list with all captured matches.
    ///
    /// # Fields
    ///
    /// * `pattern` - Regular expression applied to each current value.
    /// * `format` - Output template used for every regex match.
    ///
    ///   - `{1}` is replaced by capture group 1, `{2}` by group 2, etc.
    ///   - named placeholders such as `{query_url}` or `{request_url}` are resolved from runtime params.
    RegexFindAll {
        /// Regular expression applied to each current value.
        pattern: String,
        /// Output template used for one regex match.
        format: String,
    },

    /// Appends the current request URL to the value list.
    GetRequestUrl,

    /// Appends the raw HTTP response body to the value list when available.
    GetResponseBody,

    /// Appends a constant suffix to every current value.
    ///
    /// # Fields
    ///
    /// * `argument` - Suffix appended after each current value.
    Suffix {
        /// Suffix appended to every current value.
        argument: String,
    },

    /// Keeps only the highest positive integer from the current value list.
    Max,

    /// Resolves every current value as a URL relative to the fetched page URL and
    /// replaces relative paths with absolute URLs when possible.
    ResolveUrl,

    /// Resolves every current value relative to an ancestor of the fetched page URL.
    ResolveUrlFromParent {
        /// Number of trailing path segments removed from the request URL before resolving.
        levels: usize,
    },

    /// Replaces every current value with its parsed URL host when possible.
    GetUrlHost,

    /// Converts each current value into a public Next.js `/_next/data/...json` URL
    /// using the build id extracted from the current HTML response body.
    ///
    /// # Fields
    ///
    /// * `data_root` - Optional URL path prefix inserted before `/_next/data`.
    /// * `route_prefix` - Optional path prefix inserted between the build id and the page path.
    /// * `page_path_prefix_to_strip` - Optional prefix removed from the current page path
    ///   before the JSON path is assembled.
    BuildNextjsDataUrl {
        /// URL path prefix inserted before `/_next/data`, such as `/rtlplay`.
        #[serde(default)]
        data_root: Option<String>,
        /// Path prefix inserted between the build id and the page path, such as `detail`.
        #[serde(default)]
        route_prefix: Option<String>,
        /// Prefix removed from the public page path before the JSON path is assembled.
        #[serde(default)]
        page_path_prefix_to_strip: Option<String>,
    },

    /// Multiplies a numeric `value` by a `argument`
    ///
    /// # Fields
    /// * `argument`: the multiplier as a string.
    Ratio { argument: f64 },

    /// Formats each current value using a string template.
    ///
    /// The template replaces:
    /// - `{}` with the current value when one exists,
    /// - `{request_url}` with the fetched request URL,
    /// - named placeholders such as `{base_url}` or `{locale}` using runtime params.
    FormatText { argument: String },

    /// Builds a URL from multiple JSON fields extracted from the response.
    ///
    /// This is useful for constructing 6play URLs like `{base_url}/{program_code}-p_{program_id}/{clip_code}-c_{clip_id}`.
    BuildUrl {
        /// Base URL template.
        base: String,
        /// Field mappings: target placeholder -> JSON pointer path.
        fields: HashMap<String, String>,
    },

    /// Extracts a field from the JSON response and replaces the value list.
    ///
    /// This allows building URLs that combine multiple fields from the response.
    ExtractField {
        /// JSON pointer path to extract from the response.
        path: String,
    },

    /// Normalizes supported date formats into `YYYY-MM-DD`.
    ///
    /// # Fields
    ///
    /// * `format` - One or more input formats tried in order.
    /// * `months` - Optional month lookup required by `dd_month_yyyy`.
    GetDate {
        /// One or more input formats tried in order.
        format: GetDateSources,
        /// Month lookup used by `dd_month_yyyy`.
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        months: HashMap<String, u32>,
    },

    /// Parses human-readable durations such as `39 min` or `1 h 05 min` into seconds.
    NormalizeDuration,
}

impl ScraperAction {
    /// Applies one extraction step to the selected node and current values.
    ///
    /// # Arguments
    ///
    /// * `selected` - HTML node targeted by this entry, when a selector matched.
    /// * `texts` - Values produced by the previous actions in the pipeline.
    /// * `params` - Runtime template values available to formatting actions.
    /// * `request_url` - Final URL used to fetch the current page.
    /// * `response_body` - Raw response body of the current request when available.
    /// * `response_json` - Parsed JSON response when available (for JSON scrapers).
    pub fn apply(
        &self,
        selected: &Option<scraper::ElementRef<'_>>,
        texts: Vec<String>,
        params: &HashMap<String, String>,
        request_url: &str,
        response_body: Option<&str>,
        response_json: Option<&serde_json::Value>,
    ) -> Vec<String> {
        match self {
            ScraperAction::GetText => Self::apply_get_text(selected, texts),
            ScraperAction::HtmlToText => Self::apply_html_to_text(selected, texts),
            ScraperAction::GetAttribut { argument } => {
                Self::apply_get_attribut(selected, texts, argument)
            }
            ScraperAction::Split { argument } => Self::apply_split(texts, argument),
            ScraperAction::Map { argument, default } => Self::apply_map(texts, argument, default),
            ScraperAction::RegexFindAll { pattern, format } => {
                Self::apply_regex_find_all(texts, pattern, format, params, request_url)
            }
            ScraperAction::GetRequestUrl => Self::apply_get_request_url(texts, request_url),
            ScraperAction::GetResponseBody => Self::apply_get_response_body(texts, response_body),
            ScraperAction::Suffix { argument } => Self::apply_suffix(texts, argument),
            ScraperAction::Max => Self::apply_max(texts),
            ScraperAction::ResolveUrl => Self::apply_resolve_url(texts, request_url),
            ScraperAction::ResolveUrlFromParent { levels } => {
                Self::apply_resolve_url_from_parent(texts, request_url, *levels)
            }
            ScraperAction::GetUrlHost => Self::apply_get_url_host(texts),
            ScraperAction::BuildNextjsDataUrl {
                data_root,
                route_prefix,
                page_path_prefix_to_strip,
            } => Self::apply_build_nextjs_data_url(
                texts,
                request_url,
                response_body,
                data_root.as_deref(),
                route_prefix.as_deref(),
                page_path_prefix_to_strip.as_deref(),
            ),
            ScraperAction::Ratio { argument } => Self::apply_ration(texts, argument),
            ScraperAction::ExtractField { path } => Self::apply_extract_field(path, response_json),
            ScraperAction::BuildUrl { base, fields } => {
                Self::apply_build_url(response_json, base, fields, params)
            }
            ScraperAction::FormatText { argument } => {
                Self::apply_format_text(texts, argument, params, request_url)
            }
            ScraperAction::GetDate { format, months } => {
                Self::apply_get_date(texts, format, months)
            }
            ScraperAction::NormalizeDuration => Self::apply_normalize_duration(texts),
        }
    }

    /// Validates one configured action before scraper execution.
    ///
    /// # Arguments
    ///
    /// * `name` - Field or sub-query name that owns the action.
    /// * `owner` - Human-readable owner kind used in validation errors.
    pub(crate) fn validate(&self, name: &str, owner: &str) -> Result<()> {
        match self {
            ScraperAction::RegexFindAll { pattern, .. } => {
                Regex::new(pattern)
                    .with_context(|| format!("Invalid regex pattern for {} {}", owner, name))?;
            }
            ScraperAction::Map { argument, default } => {
                for (key, value) in argument {
                    if Self::yaml_scalar_to_optional_string(key).is_none() {
                        bail!(
                            "map for {} {} only supports scalar keys (string, bool, number, or null)",
                            owner,
                            name
                        );
                    }

                    if Self::yaml_scalar_to_optional_string(value).is_none() {
                        bail!(
                            "map for {} {} only supports scalar values (string, bool, number, or null)",
                            owner,
                            name
                        );
                    }
                }

                if let Some(default) = default {
                    if Self::yaml_scalar_to_optional_string(default).is_none() {
                        bail!(
                            "map for {} {} only supports scalar default values (string, bool, number, or null)",
                            owner,
                            name
                        );
                    }
                }
            }
            ScraperAction::GetDate { format, months } => {
                if format.is_empty() {
                    bail!(
                        "get_date for {} {} requires at least one format",
                        owner,
                        name
                    );
                }

                if format.contains_named_month_source() && months.is_empty() {
                    bail!(
                        "get_date for {} {} requires a non-empty months map when using dd_month_yyyy",
                        owner,
                        name
                    );
                }

                for (month_name, month_number) in months {
                    if month_name.trim().is_empty() {
                        bail!(
                            "get_date for {} {} contains an empty month name",
                            owner,
                            name
                        );
                    }

                    if !(1..=12).contains(month_number) {
                        bail!(
                            "get_date for {} {} contains invalid month number {} for {}",
                            owner,
                            name,
                            month_number,
                            month_name
                        );
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn apply_html_to_text(
        selected: &Option<scraper::ElementRef<'_>>,
        texts: Vec<String>,
    ) -> Vec<String> {
        let options = Self::text_only_markdown_options();

        if let Some(el) = selected {
            // HTML scraper path: convert the selected element's HTML to plain text.
            let html = Self::preprocess_html_breaks(&el.html());
            let text = Self::html_to_plain_text(&html, &options);
            return if text.is_empty() {
                Vec::new()
            } else {
                vec![text]
            };
        }

        // JSON scraper path (or any other source that already produced string values):
        // convert every existing value that contains HTML into plain text.
        texts
            .into_iter()
            .map(|value| {
                let html = Self::preprocess_html_breaks(&value);
                Self::html_to_plain_text(&html, &options)
            })
            .filter(|value| !value.is_empty())
            .collect()
    }

    /// Returns a `MarkdownOptions` instance that disables every markdown-producing
    /// feature so the converter behaves like a plain text extractor.
    fn text_only_markdown_options() -> quick_html2md::MarkdownOptions {
        quick_html2md::MarkdownOptions::new()
            .preserve_headings(false)
            .include_links(false)
            .include_images(false)
            .preserve_emphasis(false)
            .preserve_strikethrough(false)
            .preserve_lists(true)
            .preserve_code(false)
            .preserve_blockquotes(false)
            .preserve_tables(true)
    }

    /// Replaces `<br>`, `<br/>`, and `<br />` tags with newline markers before
    /// conversion so the resulting text keeps the original line breaks.
    fn preprocess_html_breaks(html: &str) -> String {
        static BR_REGEX: OnceLock<Regex> = OnceLock::new();
        let regex = BR_REGEX.get_or_init(|| {
            Regex::new(r"(?i)<br\s*/?>").expect("Invalid <br> regex")
        });
        regex.replace_all(html, "\n").into_owned()
    }

    /// Converts an HTML fragment to plain text using `quick_html2md` with all
    /// markdown features disabled, then normalizes whitespace and trims the result.
    fn html_to_plain_text(
        html: &str,
        options: &quick_html2md::MarkdownOptions,
    ) -> String {
        if html.trim().is_empty() {
            return String::new();
        }

        let markdown = quick_html2md::html_to_markdown_with_options(html, options);

        let mut result = String::new();
        for line in markdown.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(trimmed);
        }

        result
    }
    
    fn apply_get_text(
        selected: &Option<scraper::ElementRef<'_>>,
        texts: Vec<String>,
    ) -> Vec<String> {
        let mut texts = texts;
        if let Some(value) = selected
            .as_ref()
            .map(|el| el.text().collect::<String>().trim().to_string())
        {
            texts.push(value);
        }

        texts
    }

    fn apply_get_attribut(
        selected: &Option<scraper::ElementRef<'_>>,
        texts: Vec<String>,
        argument: &str,
    ) -> Vec<String> {
        let mut texts = texts;
        if let Some(value) = selected
            .as_ref()
            .and_then(|el| el.value().attr(argument).map(String::from))
        {
            texts.push(value);
        }

        texts
    }

    fn apply_split(texts: Vec<String>, separator: &str) -> Vec<String> {
        // Split expands the values already collected by previous actions.
        let mut new_texts: Vec<String> = Vec::new();
        for entry1 in texts {
            for entry2 in entry1
                .split(separator)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
            {
                new_texts.push(entry2);
            }
        }

        new_texts
    }

    fn apply_map(
        texts: Vec<String>,
        map: &YamlMapping,
        default: &Option<YamlValue>,
    ) -> Vec<String> {
        if texts.is_empty() {
            return match Self::find_map_value(map, None, "") {
                Some(Some(mapped)) => vec![mapped],
                Some(None) | None => Vec::new(),
            };
        }

        texts
            .into_iter()
            .filter_map(
                |value| match Self::find_map_value(map, Some(&value), &value) {
                    Some(Some(mapped)) => Some(mapped),
                    Some(None) => None,
                    None => match default {
                        Some(template) => Self::yaml_scalar_to_optional_string(template)
                            .unwrap_or(Some(value.clone()))
                            .map(|template| template.replace("{}", &value)),
                        None => Some(value),
                    },
                },
            )
            .collect()
    }

    fn find_map_value(
        map: &YamlMapping,
        expected_key: Option<&str>,
        original_value: &str,
    ) -> Option<Option<String>> {
        map.iter().find_map(|(key, value)| {
            let normalized_key = Self::yaml_scalar_to_optional_string(key)?;
            if normalized_key.as_deref() != expected_key {
                return None;
            }

            Self::yaml_scalar_to_optional_string(value)
                .map(|mapped| mapped.map(|mapped| mapped.replace("{}", original_value)))
        })
    }

    fn yaml_scalar_to_optional_string(value: &YamlValue) -> Option<Option<String>> {
        match value {
            YamlValue::Null => Some(None),
            YamlValue::Bool(value) => Some(Some(value.to_string())),
            YamlValue::Number(value) => Some(Some(value.to_string())),
            YamlValue::String(value) => Some(Some(value.clone())),
            YamlValue::Tagged(tagged) => Self::yaml_scalar_to_optional_string(&tagged.value),
            YamlValue::Sequence(_) | YamlValue::Mapping(_) => None,
        }
    }

    fn apply_regex_find_all(
        texts: Vec<String>,
        pattern: &str,
        format: &str,
        params: &HashMap<String, String>,
        request_url: &str,
    ) -> Vec<String> {
        let Some(regex) = Self::get_cached_regex(pattern) else {
            return texts;
        };

        let placeholder_re = PLACEHOLDER_REGEX
            .get_or_init(|| Regex::new(r"\{([A-Za-z0-9_]+)\}").expect("Invalid placeholder regex"));

        // Important: we intentionally do not use query_helpers::replace_template_placeholders
        // here, because we also want `{1}`, `{2}`, ... to map to regex capture groups.
        let mut out: Vec<String> = Vec::new();

        for value in texts {
            for captures in regex.captures_iter(&value) {
                let rendered = placeholder_re
                    .replace_all(format, |caps: &regex::Captures| {
                        let key = &caps[1];

                        // Numeric placeholders: {1}, {2}, ...
                        if key.chars().all(|c| c.is_ascii_digit()) {
                            if let Ok(index) = key.parse::<usize>() {
                                return captures
                                    .get(index)
                                    .map(|m| m.as_str().trim().to_string())
                                    .unwrap_or_default();
                            }
                            return String::new();
                        }

                        // Named placeholders
                        if key == "request_url" {
                            return request_url.to_string();
                        }

                        params
                            .get(key)
                            .cloned()
                            // Keep verbatim when missing (debug-friendly / consistent behavior)
                            .unwrap_or_else(|| caps[0].to_string())
                    })
                    .to_string();

                let trimmed = rendered.trim();
                if !trimmed.is_empty() {
                    out.push(trimmed.to_string());
                }
            }
        }

        out
    }

    fn get_cached_regex(pattern: &str) -> Option<Regex> {
        let cache = REGEX_CACHE.get_or_init(|| Mutex::new(HashMap::new()));

        if let Ok(cache_guard) = cache.lock() {
            if let Some(regex) = cache_guard.get(pattern) {
                return Some(regex.clone());
            }
        }

        let compiled = Regex::new(pattern).ok()?;

        if let Ok(mut cache_guard) = cache.lock() {
            cache_guard.insert(pattern.to_string(), compiled.clone());
        }

        Some(compiled)
    }

    fn apply_suffix(texts: Vec<String>, suffix: &str) -> Vec<String> {
        texts
            .into_iter()
            .map(|value| format!("{}{}", value, suffix))
            .collect()
    }

    fn apply_max(texts: Vec<String>) -> Vec<String> {
        texts
            .into_iter()
            .filter_map(|value| value.trim().parse::<usize>().ok())
            .filter(|value| *value > 0)
            .max()
            .map(|value| vec![value.to_string()])
            .unwrap_or_default()
    }

    fn apply_extract_field(path: &str, response_json: Option<&serde_json::Value>) -> Vec<String> {
        let mut extracted = Vec::new();
        if let Some(json) = response_json {
            if let Some(value) = json.pointer(path) {
                match value {
                    serde_json::Value::String(s) => extracted.push(s.clone()),
                    serde_json::Value::Number(n) => {
                        if let Some(n) = n.as_i64() {
                            extracted.push(n.to_string());
                        }
                    }
                    serde_json::Value::Null => {}
                    other => extracted.push(other.to_string()),
                }
            }
        }
        extracted
    }

    fn apply_build_url(
        response_json: Option<&serde_json::Value>,
        base: &str,
        fields: &HashMap<String, String>,
        params: &HashMap<String, String>,
    ) -> Vec<String> {
        let mut runtime_params = params.clone();

        // First build a set of field values from JSON response
        if let Some(json) = response_json {
            for (placeholder, path) in fields {
                if let Some(value) = json.pointer(path) {
                    let extracted = match value {
                        serde_json::Value::String(s) => s.clone(),
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::Null => String::new(),
                        other => other.to_string(),
                    };
                    runtime_params.insert(placeholder.to_string(), extracted);
                }
            }
        }

        // Now use replace_template_placeholders to handle all placeholders at once
        let (result, _missing) =
            query_helpers::replace_template_placeholders(base, &runtime_params);
        vec![result]
    }

    fn apply_get_request_url(mut texts: Vec<String>, request_url: &str) -> Vec<String> {
        texts.push(request_url.to_string());
        texts
    }

    fn apply_get_response_body(mut texts: Vec<String>, response_body: Option<&str>) -> Vec<String> {
        if let Some(response_body) = response_body {
            texts.push(response_body.to_string());
        }

        texts
    }

    fn apply_resolve_url(texts: Vec<String>, request_url: &str) -> Vec<String> {
        let base_url = Url::parse(request_url).ok();

        texts
            .into_iter()
            .map(|value| {
                if let Ok(url) = Url::parse(&value) {
                    return url.to_string();
                }

                match &base_url {
                    Some(base_url) => Self::resolve_relative_url(base_url, &value)
                        .map(|url| url.to_string())
                        .unwrap_or(value),
                    _none => value,
                }
            })
            .collect()
    }

    fn apply_resolve_url_from_parent(
        texts: Vec<String>,
        request_url: &str,
        levels: usize,
    ) -> Vec<String> {
        let base_url = Url::parse(request_url)
            .ok()
            .and_then(|url| Self::ancestor_base_url(&url, levels));

        texts
            .into_iter()
            .map(|value| {
                if let Ok(url) = Url::parse(&value) {
                    return url.to_string();
                }

                match &base_url {
                    Some(base_url) => base_url
                        .join(&value)
                        .map(|url| url.to_string())
                        .unwrap_or(value),
                    None => value,
                }
            })
            .collect()
    }

    fn apply_get_url_host(texts: Vec<String>) -> Vec<String> {
        texts
            .into_iter()
            .filter_map(|value| {
                let url = Url::parse(&value).ok()?;
                let host = url.host_str()?.trim_start_matches("www.").trim();
                if host.is_empty() {
                    None
                } else {
                    Some(host.to_string())
                }
            })
            .collect()
    }

    fn resolve_relative_url(base_url: &Url, value: &str) -> Option<Url> {
        if Self::should_resolve_against_directory(base_url, value) {
            if let Some(directory_base_url) = Self::directory_base_url(base_url) {
                if let Ok(url) = directory_base_url.join(value) {
                    return Some(url);
                }
            }
        }

        base_url.join(value).ok()
    }

    fn should_resolve_against_directory(base_url: &Url, value: &str) -> bool {
        if value.is_empty()
            || value.starts_with('/')
            || value.starts_with('?')
            || value.starts_with('#')
        {
            return false;
        }

        let path = base_url.path();
        if path.ends_with('/') {
            return false;
        }

        base_url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .map(|segment| !segment.is_empty() && !segment.contains('.'))
            .unwrap_or(false)
    }

    fn directory_base_url(base_url: &Url) -> Option<Url> {
        let mut directory_base_url = base_url.clone();
        let path = directory_base_url.path().to_string();
        directory_base_url.set_path(&format!("{}/", path));
        Some(directory_base_url)
    }

    fn ancestor_base_url(base_url: &Url, levels: usize) -> Option<Url> {
        let mut ancestor_base_url = base_url.clone();
        let mut segments = ancestor_base_url
            .path()
            .trim_matches('/')
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();

        for _ in 0..levels {
            if segments.pop().is_none() {
                break;
            }
        }

        let path = if segments.is_empty() {
            "/".to_string()
        } else {
            format!("/{}/", segments.join("/"))
        };

        ancestor_base_url.set_path(&path);
        Some(ancestor_base_url)
    }

    fn apply_build_nextjs_data_url(
        texts: Vec<String>,
        request_url: &str,
        response_body: Option<&str>,
        data_root: Option<&str>,
        route_prefix: Option<&str>,
        page_path_prefix_to_strip: Option<&str>,
    ) -> Vec<String> {
        let Some(build_id) = response_body.and_then(Self::extract_nextjs_build_id) else {
            return texts;
        };

        texts
            .into_iter()
            .map(|value| {
                Self::build_nextjs_data_url(
                    &value,
                    request_url,
                    &build_id,
                    data_root,
                    route_prefix,
                    page_path_prefix_to_strip,
                )
                .unwrap_or(value)
            })
            .collect()
    }

    fn apply_ration(texts: Vec<String>, ratio: &f64) -> Vec<String> {
        let mut new_texts: Vec<String> = Vec::new();

        for value in texts {
            // Trim only for parsing (no need to create new Strings unnecessarily)
            let value_trimmed = value.trim();

            // Try to parse value as f64
            if let Ok(val_f64) = value_trimmed.parse::<f64>() {
                let result = val_f64 * ratio;
                let rounded = result.round();
                new_texts.push(rounded.to_string());
            } else {
                // If value cannot be converted, return the original String unchanged
                new_texts.push(value);
            }
        }

        new_texts
    }

    fn apply_format_text(
        texts: Vec<String>,
        format: &str,
        params: &HashMap<String, String>,
        request_url: &str,
    ) -> Vec<String> {
        let mut new_texts: Vec<String> = Vec::new();
        let (format, _missing_keys) = query_helpers::replace_template_placeholders(format, params);
        let format = format.replace("{request_url}", request_url);

        if !texts.is_empty() {
            for value in texts {
                new_texts.push(format.replace("{}", &value));
            }
        } else {
            new_texts.push(format);
        }

        new_texts
    }

    fn apply_get_date(
        texts: Vec<String>,
        format: &GetDateSources,
        months: &HashMap<String, u32>,
    ) -> Vec<String> {
        texts
            .into_iter()
            .map(|value| Self::get_date(&value, format, months).unwrap_or(value))
            .collect()
    }

    fn get_date(
        value: &str,
        format: &GetDateSources,
        months: &HashMap<String, u32>,
    ) -> Option<String> {
        let trimmed = value.trim();

        format.find_map(|format| match format {
            GetDateSource::YyyyMmDd => Self::normalize_iso_like_date(trimmed),
            GetDateSource::DdMmYyyy => Self::normalize_day_month_year_date(trimmed),
            GetDateSource::DdMmYy => Self::normalize_day_month_short_year_date(trimmed),
            GetDateSource::DdMonthYyyy => Self::normalize_named_month_date(trimmed, months),
            GetDateSource::DaysFromToday => Self::normalize_days_from_today(trimmed),
            GetDateSource::YyyyMmDdHhMmSs => Self::normalize_iso_like_date_time(trimmed),
        })
    }

    fn normalize_iso_like_date(value: &str) -> Option<String> {
        let captures = Self::iso_like_date_regex().captures(value)?;
        let year = captures.get(2)?.as_str().parse::<u32>().ok()?;
        let month = captures.get(3)?.as_str().parse::<u32>().ok()?;
        let day = captures.get(4)?.as_str().parse::<u32>().ok()?;

        Self::format_iso_date(year, month, day)
    }

    fn normalize_iso_like_date_time(value: &str) -> Option<String> {
        if let Ok(date_time) = DateTime::parse_from_rfc3339(value) {
            return Some(date_time.format("%F %T").to_string());
        }

        if let Ok(date_time) = NaiveDateTime::parse_from_str(value, "%F %T") {
            return Some(date_time.format("%F %T").to_string());
        }

        let captures = Self::iso_like_date_time_regex().captures(value)?;
        let year = captures.get(2)?.as_str().parse::<u32>().ok()?;
        let month = captures.get(3)?.as_str().parse::<u32>().ok()?;
        let day = captures.get(4)?.as_str().parse::<u32>().ok()?;
        let hour = captures.get(5)?.as_str().parse::<u32>().ok()?;
        let min = captures.get(6)?.as_str().parse::<u32>().ok()?;
        let sec = captures.get(7)?.as_str().parse::<u32>().ok()?;

        if !Self::is_valid_date(year, month, day) {
            return None;
        }

        if hour > 23 || min > 59 || sec > 59 {
            return None;
        }

        Some(format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            year, month, day, hour, min, sec
        ))
    }

    fn normalize_day_month_year_date(value: &str) -> Option<String> {
        let captures = Self::day_month_year_regex().captures(value)?;
        let day = captures.get(1)?.as_str().parse::<u32>().ok()?;
        let month = captures.get(2)?.as_str().parse::<u32>().ok()?;
        let year = captures.get(3)?.as_str().parse::<u32>().ok()?;

        Self::format_iso_date(year, month, day)
    }

    fn normalize_day_month_short_year_date(value: &str) -> Option<String> {
        let captures = Self::day_month_short_year_regex().captures(value)?;
        let day = captures.get(1)?.as_str().parse::<u32>().ok()?;
        let month = captures.get(2)?.as_str().parse::<u32>().ok()?;
        let short_year = captures.get(3)?.as_str().parse::<u32>().ok()?;
        let year = if short_year >= 70 {
            1900 + short_year
        } else {
            2000 + short_year
        };

        Self::format_iso_date(year, month, day)
    }

    fn normalize_named_month_date(value: &str, months: &HashMap<String, u32>) -> Option<String> {
        let normalized = Self::normalize_date_text(value);
        let tokens = normalized
            .split_whitespace()
            .filter(|token| !token.is_empty())
            .collect::<Vec<_>>();

        if tokens.len() < 3 {
            return None;
        }

        let date_tokens = &tokens[tokens.len().saturating_sub(3)..];
        let day = date_tokens.first()?.parse::<u32>().ok()?;
        let month = Self::lookup_month(months, date_tokens.get(1)?)?;
        let year = date_tokens.get(2)?.parse::<u32>().ok()?;

        Self::format_iso_date(year, month, day)
    }

    fn normalize_days_from_today(value: &str) -> Option<String> {
        let days = value.trim().parse::<i64>().ok()?;
        let today = Local::now().date_naive();
        let target_date = today.checked_add_signed(Duration::days(days))?;

        Some(target_date.format("%F").to_string())
    }

    fn lookup_month(months: &HashMap<String, u32>, token: &str) -> Option<u32> {
        let normalized_token = Self::normalize_date_text(token);

        months.iter().find_map(|(month_name, month_number)| {
            (Self::normalize_date_text(month_name) == normalized_token).then_some(*month_number)
        })
    }

    fn normalize_date_text(value: &str) -> String {
        value
            .trim()
            .to_lowercase()
            .replace([',', '.'], " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn format_iso_date(year: u32, month: u32, day: u32) -> Option<String> {
        if !Self::is_valid_date(year, month, day) {
            return None;
        }

        Some(format!("{:04}-{:02}-{:02}", year, month, day))
    }

    fn is_valid_date(year: u32, month: u32, day: u32) -> bool {
        if year == 0 || !(1..=12).contains(&month) {
            return false;
        }

        let max_day = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if Self::is_leap_year(year) => 29,
            2 => 28,
            _ => return false,
        };

        (1..=max_day).contains(&day)
    }

    fn is_leap_year(year: u32) -> bool {
        (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
    }

    fn apply_normalize_duration(texts: Vec<String>) -> Vec<String> {
        texts
            .into_iter()
            .map(|value| Self::normalize_duration(&value).unwrap_or(value))
            .collect()
    }

    fn normalize_duration(value: &str) -> Option<String> {
        Self::parse_duration_seconds(value).map(|seconds| seconds.to_string())
    }

    fn parse_duration_seconds(value: &str) -> Option<u64> {
        let regex = Self::duration_regex();

        let mut total_seconds: u64 = 0;
        let mut matched = false;

        for captures in regex.captures_iter(value) {
            let amount = captures.get(1)?.as_str().parse::<u64>().ok()?;
            let unit = captures.get(2)?.as_str().to_lowercase();

            matched = true;
            match unit.as_str() {
                "h" => total_seconds += amount * 3600,
                "min" | "m" => total_seconds += amount * 60,
                "sec" | "s" => total_seconds += amount,
                _ => return None,
            }
        }

        if !matched {
            return None;
        }

        Some(total_seconds)
    }

    fn iso_like_date_regex() -> &'static Regex {
        static REGEX: OnceLock<Regex> = OnceLock::new();
        REGEX.get_or_init(|| {
            Regex::new(r"(^|[^0-9])(\d{4})-(\d{2})-(\d{2})([^0-9]|$)")
                .expect("Invalid ISO-like date regex")
        })
    }

    fn iso_like_date_time_regex() -> &'static Regex {
        static REGEX: OnceLock<Regex> = OnceLock::new();
        REGEX.get_or_init(|| {
            Regex::new(r"(^|[^0-9])(\d{4})-(\d{2})-(\d{2})[ T](\d{2}):(\d{2}):(\d{2})(?:Z|[+-]\d{2}:?\d{2})?([^0-9]|$)")
                .expect("Invalid ISO-like date time regex")
        })
    }

    fn day_month_year_regex() -> &'static Regex {
        static REGEX: OnceLock<Regex> = OnceLock::new();
        REGEX.get_or_init(|| {
            Regex::new(r"\b(\d{1,2})[/.](\d{1,2})[/.](\d{4})\b")
                .expect("Invalid day/month/year date regex")
        })
    }

    fn day_month_short_year_regex() -> &'static Regex {
        static REGEX: OnceLock<Regex> = OnceLock::new();
        REGEX.get_or_init(|| {
            Regex::new(r"\b(\d{1,2})[/.](\d{1,2})[/.](\d{2})\b")
                .expect("Invalid day/month/short-year date regex")
        })
    }

    fn duration_regex() -> &'static Regex {
        static REGEX: OnceLock<Regex> = OnceLock::new();
        REGEX.get_or_init(|| {
            Regex::new(r"(?i)(\d+)\s*(h|min|m|sec|s)?").expect("Invalid duration regex")
        })
    }

    fn nextjs_build_id_json_regex() -> &'static Regex {
        static REGEX: OnceLock<Regex> = OnceLock::new();
        REGEX.get_or_init(|| {
            Regex::new(r#""buildId":"([^"]+)""#).expect("Invalid Next.js buildId JSON regex")
        })
    }

    fn nextjs_build_manifest_regex() -> &'static Regex {
        static REGEX: OnceLock<Regex> = OnceLock::new();
        REGEX.get_or_init(|| {
            Regex::new(r#"/_next/static/([^/]+)/_buildManifest\.js"#)
                .expect("Invalid Next.js build manifest regex")
        })
    }

    fn build_nextjs_data_url(
        value: &str,
        request_url: &str,
        build_id: &str,
        data_root: Option<&str>,
        route_prefix: Option<&str>,
        page_path_prefix_to_strip: Option<&str>,
    ) -> Option<String> {
        let request_url = Url::parse(request_url).ok()?;
        let page_url = Url::parse(value)
            .or_else(|_| request_url.join(value))
            .ok()?;

        if page_url.path().contains("/_next/data/") && page_url.path().ends_with(".json") {
            return Some(page_url.to_string());
        }

        let page_path = Self::strip_path_prefix(
            page_url.path().trim_start_matches('/'),
            page_path_prefix_to_strip,
        );
        let data_root = Self::normalize_root_path(data_root);
        let route_prefix = Self::normalize_path_segment(route_prefix);

        let mut json_path = format!("{}/_next/data/{}", data_root, build_id);
        if let Some(route_prefix) = route_prefix {
            json_path.push('/');
            json_path.push_str(&route_prefix);
        }
        if !page_path.is_empty() {
            json_path.push('/');
            json_path.push_str(page_path);
        }
        json_path.push_str(".json");

        let mut json_url = format!("{}{}", page_url.origin().ascii_serialization(), json_path);
        if let Some(query) = page_url.query() {
            json_url.push('?');
            json_url.push_str(query);
        }

        Some(json_url)
    }

    fn extract_nextjs_build_id(response_body: &str) -> Option<String> {
        for pattern in [
            Self::nextjs_build_id_json_regex(),
            Self::nextjs_build_manifest_regex(),
        ] {
            if let Some(captures) = pattern.captures(response_body) {
                if let Some(build_id) = captures.get(1) {
                    return Some(build_id.as_str().to_string());
                }
            }
        }

        None
    }

    fn normalize_root_path(value: Option<&str>) -> String {
        value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| format!("/{}", value.trim_matches('/')))
            .unwrap_or_default()
    }

    fn normalize_path_segment(value: Option<&str>) -> Option<String> {
        value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.trim_matches('/').to_string())
            .filter(|value| !value.is_empty())
    }

    fn strip_path_prefix<'a>(path: &'a str, prefix: Option<&str>) -> &'a str {
        let Some(prefix) = prefix
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.trim_matches('/').to_string())
        else {
            return path;
        };

        path.strip_prefix(prefix.as_str())
            .unwrap_or(path)
            .trim_start_matches('/')
    }
}
