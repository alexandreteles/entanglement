#![doc = include_str!("../README.md")]
//!
//! # Code organization
//!
//! - [`cli`] defines commands, arguments, and options.
//! - [`commands`] runs the selected command.
//! - [`input`] finds source files and applies patches in memory.
//! - [`analysis`] parses source files and calculates metrics.
//! - [`languages`] supplies language rules and syntax analysis.
//! - [`metrics`] calculates code size, complexity, and maintainability.
//! - [`graph`] builds program graphs from source facts.
//! - [`resolver`] resolves source references.
//! - [`snapshot`] stores file contents for one analysis run.
//! - [`structure`] analyzes source dependencies and declaration changes.
//! - [`model`] defines report data types.
//! - [`report_delta`] compares metrics before and after a change.
//! - [`ui`] formats reports as text or JSON.

mod analysis;
mod cli;
mod commands;
mod graph;
mod input;
mod languages;
mod metrics;
mod model;
mod report_delta;
mod resolver;
mod snapshot;
mod structure;
mod ui;

use metrics::selection::Selection;

type Error = Box<dyn std::error::Error + Send + Sync>;
type Result<T> = std::result::Result<T, Error>;

/// Run the selected command and write its report to standard output.
///
/// Report input, parse, and output errors on standard error with a failed exit.
fn main() -> std::process::ExitCode {
    use clap::Parser;
    let arguments = cli::Cli::parse();
    let selection = Selection::new(&arguments.metrics);
    let report =
        commands::run(arguments.command, selection).and_then(|result| match arguments.format {
            cli::OutputFormat::Human => Ok(ui::human::render(&result, selection)),
            cli::OutputFormat::Json => ui::json::render(&result, selection),
        });
    match report {
        Ok(report) => {
            use std::io::Write;
            match writeln!(std::io::stdout().lock(), "{report}") {
                Ok(()) => std::process::ExitCode::SUCCESS,
                Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => {
                    std::process::ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("Error: {error}");
                    std::process::ExitCode::FAILURE
                }
            }
        }
        Err(error) => {
            eprintln!("Error: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
