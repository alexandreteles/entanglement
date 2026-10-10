mod analysis;
mod cli;
mod commands;
mod input;
mod languages;
mod metrics;
mod model;
mod report_delta;
mod resolver;
mod snapshot;
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
