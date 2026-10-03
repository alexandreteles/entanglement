//! Define the command line for file, repository, patch, and candidate analysis.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

/// Command-line arguments for the analyzer.
///
/// Select one command and an output format. `--format` is accepted before or
/// after the command. The default format is `human`.
#[derive(Debug, Parser)]
#[command(name = "entanglement", version, about = "Analyze Rust code metrics")]
pub struct Cli {
    /// Select the result format.
    #[arg(long, value_enum, global = true, default_value_t = OutputFormat::Human)]
    pub format: OutputFormat,

    /// Select an analysis mode.
    #[command(subcommand)]
    pub command: Command,
}

/// A supported analysis command.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Analyze one source file.
    File {
        /// The source file to analyze.
        path: PathBuf,
    },
    /// Analyze source files below a repository directory.
    Repo {
        /// The repository directory to analyze.
        path: PathBuf,
    },
    /// Apply a diff to one source file in memory and analyze the result.
    Patch {
        /// The source file before the patch.
        file: PathBuf,
        /// A unified diff file, or `-` to read the diff from standard input.
        #[arg(long)]
        diff: PathBuf,
    },
    /// Apply a diff to a file or repository in memory and analyze the candidate.
    Candidate {
        /// The source file or repository directory to analyze.
        path: PathBuf,
        /// A unified diff file, or `-` to read the diff from standard input.
        #[arg(long)]
        diff: PathBuf,
    },
}

/// An output format supported by the command line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// Render a readable terminal report.
    #[default]
    Human,
    /// Render a JSON document.
    Json,
}
