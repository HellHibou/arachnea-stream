use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping as YamlMapping, Value as YamlValue};
use std::collections::HashMap;

mod base64_decode;
mod build_nextjs_data_url;
mod build_url;
mod extract_field;
mod format_text;
mod get_attribut;
mod get_date;
mod get_request_url;
mod get_response_body;
mod get_text;
mod get_url_host;
mod html_to_text;
mod map;
mod max;
mod normalize_duration;
mod ratio;
mod regex_find_all;
mod resolve_url;
mod split;
mod suffix;

/// Runtime parameter containing the public path of the generic HTTP proxy.
pub const HTTP_PROXY_PUBLIC_PATH_PARAM: &str = "__arachnea_http_proxy_public_path";

// `GetDateSource` and `GetDateSources` are owned by the `get_date` module
// because every consumer of these types belongs to the `get_date` action.
// Re-export them so the surrounding `ScraperAction` enum can keep naming
// `GetDateSources` directly.
pub use get_date::{GetDateSource, GetDateSources};

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
    ResolveUrl {
        /// Whether resolved HTTP(S) URLs should be wrapped through the public proxy route.
        #[serde(default)]
        proxy: bool,
    },

    /// Resolves every current value relative to an ancestor of the fetched page URL.
    ResolveUrlFromParent {
        /// Number of trailing path segments removed from the request URL before resolving.
        levels: usize,
        /// Whether resolved HTTP(S) URLs should be wrapped through the public proxy route.
        #[serde(default)]
        proxy: bool,
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

    /// Base64-decodes each current value.
    ///
    /// Uses the standard base64 alphabet. Non-decodable values are kept as-is.
    #[serde(rename = "base64_decode")]
    Base64Decode,
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
            ScraperAction::GetText => get_text::apply(selected, texts),
            ScraperAction::HtmlToText => html_to_text::apply(selected, texts),
            ScraperAction::GetAttribut { argument } => {
                get_attribut::apply(selected, texts, argument)
            }
            ScraperAction::Split { argument } => split::apply(texts, argument),
            ScraperAction::Map { argument, default } => map::apply(texts, argument, default),
            ScraperAction::RegexFindAll { pattern, format } => {
                regex_find_all::apply(texts, pattern, format, params, request_url)
            }
            ScraperAction::GetRequestUrl => get_request_url::apply(texts, request_url),
            ScraperAction::GetResponseBody => get_response_body::apply(texts, response_body),
            ScraperAction::Suffix { argument } => suffix::apply(texts, argument),
            ScraperAction::Max => max::apply(texts),
            ScraperAction::ResolveUrl { proxy } => {
                resolve_url::apply(texts, request_url, params, *proxy)
            }
            ScraperAction::ResolveUrlFromParent { levels, proxy } => {
                resolve_url::apply_from_parent(texts, request_url, params, *levels, *proxy)
            }
            ScraperAction::GetUrlHost => get_url_host::apply(texts),
            ScraperAction::BuildNextjsDataUrl {
                data_root,
                route_prefix,
                page_path_prefix_to_strip,
            } => build_nextjs_data_url::apply(
                texts,
                request_url,
                response_body,
                data_root.as_deref(),
                route_prefix.as_deref(),
                page_path_prefix_to_strip.as_deref(),
            ),
            ScraperAction::Ratio { argument } => ratio::apply(texts, argument),
            ScraperAction::ExtractField { path } => extract_field::apply(path, response_json),
            ScraperAction::BuildUrl { base, fields } => {
                build_url::apply(response_json, base, fields, params)
            }
            ScraperAction::FormatText { argument } => {
                format_text::apply(texts, argument, params, request_url)
            }
            ScraperAction::GetDate { format, months } => get_date::apply(texts, format, months),
            ScraperAction::NormalizeDuration => normalize_duration::apply(texts),
            ScraperAction::Base64Decode => base64_decode::apply(texts),
        }
    }

    /// Validates one configured action before scraper execution by delegating
    /// to the matching per-action module.
    ///
    /// # Arguments
    ///
    /// * `name` - Field or sub-query name that owns the action.
    /// * `owner` - Human-readable owner kind used in validation errors.
    ///
    /// # Errors
    ///
    /// Returns an error when the action configuration is invalid (for example
    /// when a regex cannot be compiled, when the `map` table contains a
    /// non-scalar entry, or when `get_date` is missing a required format).
    pub(crate) fn validate(&self, name: &str, owner: &str) -> Result<()> {
        match self {
            ScraperAction::RegexFindAll { pattern, .. } => {
                regex_find_all::validate(name, owner, pattern)
            }
            ScraperAction::Map { argument, default } => {
                map::validate(name, owner, argument, default)
            }
            ScraperAction::GetDate { format, months } => {
                get_date::validate(name, owner, format, months)
            }
            _ => Ok(()),
        }
    }
}
