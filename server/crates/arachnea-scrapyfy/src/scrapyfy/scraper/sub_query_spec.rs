//! [`SubQuerySpec`] — aggregate data carried by every sub-query.

use std::collections::HashMap;

use crate::scrapyfy::actions::ScraperAction;
use crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode;
use crate::scrapyfy::post_processes::ScraperFieldMapping;
use crate::scrapyfy::ScraperHttpConfig;

/// Aggregate specification data carried by every sub-query.
///
/// This struct contains the configuration that defines how a sub-query operates,
/// including how to extract request URLs, how to scope execution to specific contexts,
/// and how to merge results back into the parent item.
///
/// Present on all sub-queries, absent on root queries.
#[derive(Default)]
pub struct SubQuerySpec {
    /// Path where the sub-query result should be merged.
    ///
    /// - `None`: Results are merged at the top level for sibling sub-queries,
    ///   or at the parent level for entry-level sub-queries.
    /// - `Some(path)`: Results are nested under the specified path.
    ///
    /// Path format: `"parent>child>grandchild"` creates nested objects.
    pub target: Option<String>,

    /// JSON pointer to the entry value used as the request URL.
    ///
    /// - `None`: Uses the parent row as the context for entry-level sub-queries.
    /// - `Some(pointer)`: Extracts the request URL from the specified JSON pointer.
    pub request_pointer: Option<String>,

    /// Selection mode for the request pointer.
    ///
    /// Determines whether to use the first match or all matches from the request pointer:
    /// - [`HtmlScraperSelectMode::First`]: Only the first matching value.
    /// - [`HtmlScraperSelectMode::All`]: All matching values.
    pub request_select: HtmlScraperSelectMode,

    /// Actions applied to request URL values before the HTTP fetch.
    ///
    /// These actions transform the extracted URL values before making the request.
    pub request_actions: Vec<ScraperAction>,

    /// JSON pointer in the **parent row** to scope execution to N contexts.
    ///
    /// - `None`: Single context (the entire parent row).
    /// - `Some(pointer)`: Execution is scoped to each value matching the pointer.
    ///
    /// This allows a sub-query to iterate over multiple contexts within the parent row.
    pub context_pointer: Option<String>,

    /// Selection mode for the context pointer.
    ///
    /// Determines whether to use the first match or all matches from the context pointer.
    pub context_select: HtmlScraperSelectMode,

    /// Entries applied to the context row.
    ///
    /// These entries extract data from each context row before issuing the follow-up request.
    /// The extracted values are available to the sub-query via template parameters.
    pub context_entries: Vec<Box<dyn super::ScraperEntrySpec>>,

    /// Filter on the **context row** (before fetch).
    ///
    /// Determines whether to execute the sub-query for a given context row.
    /// Format: `{"field_name": ["allowed_value_1", "allowed_value_2", ...]}`.
    /// The sub-query is only executed if the field value matches one of the allowed values.
    pub filters: HashMap<String, Vec<String>>,

    /// Filter on each **fetched row** (after fetch).
    ///
    /// Determines whether to keep a fetched row in the results.
    /// Format: Same as `filters` — `{"field_name": ["allowed_value_1", ...]}`.
    /// Rows are only kept if their field values match the allowed values.
    pub row_filters: HashMap<String, Vec<String>>,

    /// Fields from the source item copied into each generated item.
    ///
    /// Allows propagating values from the parent item to each sub-query result.
    pub copy_item_fields: Vec<ScraperFieldMapping>,

    /// HTTP configuration inherited from the parent.
    ///
    /// Can be overridden by the sub-query's own HTTP configuration.
    /// Includes settings like user agent, max redirects, timeouts, etc.
    pub http_config: ScraperHttpConfig,
}
