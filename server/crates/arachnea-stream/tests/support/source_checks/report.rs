//! Human-readable aggregation of per-source check results.

use std::io::IsTerminal;

/// Display severity for one source/operation result.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ResultKind {
    /// The operation ran and every declared assertion passed.
    Ok,
    /// A production operation or source is not covered by a contract.
    Warning,
    /// The operation failed, was blocked or could not be configured.
    Error,
}

impl ResultKind {
    fn label(self) -> &'static str {
        match self {
            Self::Ok => "Ok",
            Self::Warning => "Warning",
            Self::Error => "Error",
        }
    }

    fn color(self) -> &'static str {
        match self {
            Self::Ok => "\x1b[32m",
            Self::Warning => "\x1b[33m",
            Self::Error => "\x1b[31m",
        }
    }

    fn from_outcome(outcome: &str) -> Self {
        match outcome {
            "passed" => Self::Ok,
            "coverage_missing" | "not_tested" => Self::Warning,
            _ => Self::Error,
        }
    }
}

/// One row of the result table.
struct Row {
    source: String,
    operation: String,
    status: &'static str,
    kind: ResultKind,
    message: String,
}

/// Collects rows for every source and prints one aligned, optionally colored table.
#[derive(Default)]
pub(super) struct Report {
    rows: Vec<Row>,
}

impl Report {
    /// Records a check outcome for one source and operation.
    ///
    /// # Arguments
    /// * `source` - Production source identifier or selected path label.
    /// * `operation` - Facade operation name, or a coarse label for preflight failures.
    /// * `outcome` - Stable source-check outcome used for status and severity; nonapplicable operations are excluded.
    /// * `message` - Short diagnostic; ignored for passed operations by the renderer.
    pub fn record(
        &mut self,
        source: &str,
        operation: &str,
        outcome: &str,
        message: impl Into<String>,
    ) {
        if outcome == "not_applicable" {
            return;
        }
        self.rows.push(Row {
            source: source.to_string(),
            operation: operation.to_string(),
            status: match outcome {
                "passed" => "Passed",
                "coverage_missing" | "not_tested" => "Skip",
                "blocked" => "Blocked",
                _ => "Failed",
            },
            kind: ResultKind::from_outcome(outcome),
            message: message.into(),
        });
    }

    /// Reports whether a source already has an error row.
    pub fn has_error(&self, source: &str) -> bool {
        self.rows
            .iter()
            .any(|row| row.source == source && row.kind == ResultKind::Error)
    }

    /// Prints the accumulated rows as one table, sorted by source while preserving operation order.
    pub fn print(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        self.rows
            .sort_by(|left, right| left.source.cmp(&right.source));
        let source_width = self
            .rows
            .iter()
            .map(|row| row.source.chars().count())
            .max()
            .unwrap_or(0)
            .max("Source".len());
        let operation_width = self
            .rows
            .iter()
            .map(|row| row.operation.chars().count())
            .max()
            .unwrap_or(0)
            .max("Operation".len());
        let result_width = "Warning".len();
        let status_width = "Blocked".len();
        let color = colors_enabled();

        println!();
        println!("Source check results:");
        println!(
            "{:<source_width$}  {:<operation_width$}  {:<status_width$}  {:<result_width$}  Message",
            "Source", "Operation", "Status", "Result"
        );
        println!(
            "{:<source_width$}  {:<operation_width$}  {:<status_width$}  {:<result_width$}  {}",
            "-".repeat(source_width),
            "-".repeat(operation_width),
            "-".repeat(status_width),
            "-".repeat(result_width),
            "-".repeat("Message".len())
        );
        for row in &self.rows {
            let result = format!("{:<result_width$}", row.kind.label());
            let result = if color {
                format!("{}{result}\x1b[0m", row.kind.color())
            } else {
                result
            };
            println!(
                "{:<source_width$}  {:<operation_width$}  {:<status_width$}  {result}  {}",
                row.source, row.operation, row.status, row.message
            );
        }
        let count = |kind| self.rows.iter().filter(|row| row.kind == kind).count();
        let label = |kind: ResultKind| {
            let label = kind.label();
            if color {
                format!("{}{label}\x1b[0m", kind.color())
            } else {
                label.to_string()
            }
        };
        println!();
        println!(
            "Total: {}  {}: {}  {}: {}  {}: {}",
            self.rows.len(),
            label(ResultKind::Ok),
            count(ResultKind::Ok),
            label(ResultKind::Warning),
            count(ResultKind::Warning),
            label(ResultKind::Error),
            count(ResultKind::Error)
        );
        println!();
    }
}

/// Enables ANSI colors only for an interactive terminal without an explicit opt-out.
fn colors_enabled() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if std::env::var("CLICOLOR").is_ok_and(|value| value == "0") {
        return false;
    }
    std::io::stdout().is_terminal()
}
