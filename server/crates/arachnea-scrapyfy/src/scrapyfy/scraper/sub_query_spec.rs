//! [`SubQuerySpec`] — aggregate data carried by every sub-query.

use std::collections::HashMap;

use crate::scrapyfy::actions::ScraperAction;
use crate::scrapyfy::scraper_html::entry::HtmlScraperSelectMode;
use crate::scrapyfy::post_processes::ScraperFieldMapping;
use crate::scrapyfy::ScraperHttpConfig;

/// Spec sub-query — présente sur sub_queries, absente sur racines.
#[derive(Default)]
pub struct SubQuerySpec {
    /// Chemin où merger le résultat. `None` = top-level pour sibling, `parent` pour sub_query d'entry.
    pub target: Option<String>,

    /// Pointer vers la valeur d'entry servant d'URL de requête. `None` = `parent` pour sub_query d'entry.
    pub request_pointer: Option<String>,

    /// Sélection du pointer de requête (first ou all).
    pub request_select: HtmlScraperSelectMode,

    /// Actions appliquées sur les valeurs de requête avant fetch.
    pub request_actions: Vec<ScraperAction>,

    /// Pointer dans le **row parent** pour scoper l'exécution à N contextes.
    /// `None` = un seul context (le row parent).
    pub context_pointer: Option<String>,

    /// Sélection du context_pointer.
    pub context_select: HtmlScraperSelectMode,

    /// Entries appliquées au context.
    pub context_entries: Vec<Box<dyn super::ScraperEntrySpec>>,

    /// Filtre sur le **context row** (avant fetch). "Est-ce que je lance la requête ?"
    pub filters: HashMap<String, Vec<String>>,

    /// Filtre sur chaque **row fetched** (après fetch). "Est-ce que je garde cette row ?"
    pub row_filters: HashMap<String, Vec<String>>,

    /// Champs du source item copiés dans chaque item généré.
    pub copy_item_fields: Vec<ScraperFieldMapping>,

    /// Configuration HTTP héritée du parent (peut être surchargée par la sub_query).
    pub http_config: ScraperHttpConfig,
}
