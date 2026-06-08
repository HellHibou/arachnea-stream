//! Row extraction utilities for JSON queries.
//!
//! This module provides helpers for applying entries to individual JSON values
//! and building [`ScraperDataNode`] output.  The core entry-application logic
//! currently lives in [`super::entry`] and [`super::query`]; this module will
//! absorb row-extraction helpers as the refactor progresses.