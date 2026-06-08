//! JSON pointer utilities.
//!
//! This module provides utilities for navigating JSON values using pointer-like
//! syntax with wildcards and array filters, with output that can be used to
//! streamline rustdoc.
//!
//! > **Note** — The core pointer navigation functions [`select_json_values`] and
//! > [`json_value_to_strings`] currently live in [`super::entry`] due to their
//! > tight coupling with entry processing.  They will be migrated here when
//! > the entry module is refactored.  In the meantime this module acts as a
//! > placeholder for future pointer-related utilities.