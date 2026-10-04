mod analysis;
mod cli;
mod input;
mod languages;
mod metrics;
mod model;
mod resolver;
mod ui;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use tree_sitter::InputEdit;

type Error = Box<dyn std::error::Error + Send + Sync>;
type Result<T> = std::result::Result<T, Error>;
type SourcePaths = Box<dyn Iterator<Item = Result<PathBuf>> + Send>;

/// Run the selected command and write its report to standard output.
///
/// Report input, parse, and output errors on standard error with a failed exit.
fn main() -> std::process::ExitCode {
    use clap::Parser;
    let arguments = cli::Cli::parse();
    let report = run(arguments.command).and_then(|result| match arguments.format {
        cli::OutputFormat::Human => Ok(ui::human::render(&result)),
        cli::OutputFormat::Json => ui::json::render(&result),
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

/// Execute a file or repository analysis, or apply a virtual patch.
fn run(command: cli::Command) -> Result<model::AnalysisResult> {
    match command {
        cli::Command::File { path } => {
            let path = std::path::absolute(path)?;
            let parsed = analysis::Worker::new()?
                .analyze_path(&path)?
                .ok_or_else(|| {
                    std::io::Error::other(format!(
                        "No registered grammar supports {}",
                        path.display()
                    ))
                })?;
            let mut facts = vec![parsed.facts];
            resolver::resolve(&mut facts);
            Ok(model::AnalysisResult {
                files: facts.into_iter().map(|fact| fact.analysis).collect(),
                patch: None,
            })
        }
        cli::Command::Repo { path } => {
            let path = std::path::absolute(path)?;
            let mut facts = analysis::analyze_paths(input::discover(&path)?, |parsed| parsed.facts)?;
            resolver::resolve(&mut facts);
            Ok(model::AnalysisResult {
                files: facts.into_iter().map(|fact| fact.analysis).collect(),
                patch: None,
            })
        }
        cli::Command::Patch { file, diff } => analyze_patch(&file, &diff, true),
        cli::Command::Candidate { path, diff } => analyze_patch(&path, &diff, false),
    }
}

/// Apply a diff in memory and resolve the full before and after file sets.
///
/// A file target accepts one patch whose old path matches that file. A
/// directory target accepts patches relative to that directory. Reject empty
/// diffs and attempts to overwrite another file. Never write source bytes.
fn analyze_patch(path: &Path, diff: &Path, single: bool) -> Result<model::AnalysisResult> {
    let diff = input::read_diff(diff)?;
    let patches = input::parse_diff(&diff)?;
    let (root, paths) = patch_sources(path, &patches, single)?;
    let mut parsed = analysis::analyze_paths(paths, std::convert::identity)?;
    if single && parsed.is_empty() {
        return Err("The selected file has no supported language".into());
    }
    let mut before: Vec<_> = parsed.iter().map(|file| file.facts.clone()).collect();
    resolver::resolve(&mut before);
    let mut worker = analysis::Worker::new()?;
    let changes: Vec<_> = patches
        .iter()
        .map(|patch| apply_file_patch(&mut worker, &mut parsed, &root, patch))
        .collect::<Result<_>>()?;
    let mut after: Vec<_> = parsed.into_iter().map(|file| file.facts).collect();
    after.sort_by(|a, b| a.path.cmp(&b.path));
    resolver::resolve(&mut after);
    let files = changes
        .into_iter()
        .map(|change| change.compare(&before, &after))
        .collect();
    Ok(model::AnalysisResult {
        files: after.into_iter().map(|file| file.analysis).collect(),
        patch: Some(model::PatchAnalysis { files }),
    })
}

/// Reject empty diffs and target files changed by more than one file patch.
fn validate_patches(root: &Path, patches: &[input::FilePatch]) -> Result<()> {
    if patches.is_empty() {
        return Err("The diff has no file patches".into());
    }
    let mut touched = BTreeSet::new();
    for patch in patches {
        let targets: BTreeSet<_> = patch
            .old_path
            .iter()
            .chain(&patch.new_path)
            .map(|path| input::path_identity(&root.join(path)))
            .collect::<Result<_>>()?;
        if targets.into_iter().any(|target| !touched.insert(target)) {
            return Err("The diff changes one file target more than once".into());
        }
    }
    Ok(())
}

/// Find the diff root and source files for a file or directory target.
///
/// A file target must match one old-file path. Candidate analysis includes
/// the nearest repository, or the diff root when no repository is found.
fn patch_sources(
    path: &Path,
    patches: &[input::FilePatch],
    single: bool,
) -> Result<(PathBuf, SourcePaths)> {
    let target = std::path::absolute(path)?;
    if single && !target.is_file() {
        return Err("PATCH requires a source file".into());
    }
    let root = if target.is_file() {
        if patches.len() != 1 {
            return Err("A file target requires one file patch".into());
        }
        let old = patches[0]
            .old_path
            .as_ref()
            .ok_or("A file target requires an old-file path")?;
        if !target.ends_with(old) {
            return Err("The diff old path does not match the selected file".into());
        }
        let mut root = target.clone();
        for _ in old.components() {
            root.pop();
        }
        root
    } else {
        target.clone()
    };
    validate_patches(&root, patches)?;
    let paths: SourcePaths = if target.is_file() && single {
        Box::new(std::iter::once(Ok(target.clone())))
    } else {
        let repository = if target.is_file() {
            target
                .ancestors()
                .skip(1)
                .find(|directory| {
                    directory.join("Cargo.toml").is_file() || directory.join(".git").exists()
                })
                .unwrap_or(&root)
        } else {
            &root
        };
        let paths = input::discover(repository)?;
        let old_paths: Vec<_> = patches
            .iter()
            .filter_map(|patch| patch.old_path.as_ref())
            .map(|path| root.join(path))
            .collect();
        Box::new(paths.chain(old_paths.into_iter().map(Ok)))
    };
    Ok((root, paths))
}

/// The paths and incremental edits for one virtual file change.
struct PatchChange {
    old_path: Option<PathBuf>,
    new_path: Option<PathBuf>,
    old_target: Option<PathBuf>,
    new_target: Option<PathBuf>,
    edits: Vec<InputEdit>,
}

impl PatchChange {
    /// Compare the resolved reports and function contributions for this change.
    fn compare(
        self,
        before: &[model::FileFacts],
        after: &[model::FileFacts],
    ) -> model::FilePatchAnalysis {
        let old = self
            .old_target
            .as_ref()
            .and_then(|target| before.iter().find(|file| file.target == *target))
            .map(|file| file.analysis.clone());
        let new = self
            .new_target
            .as_ref()
            .and_then(|target| after.iter().find(|file| file.target == *target))
            .map(|file| file.analysis.clone());
        model::FilePatchAnalysis {
            path: self
                .new_path
                .or(self.old_path)
                .expect("A patch has a path")
                .display()
                .to_string(),
            functions: function_deltas(old.as_ref(), new.as_ref(), &self.edits),
            before: old,
            after: new,
        }
    }
}

/// Apply one virtual change and replace its retained source and parse trees.
///
/// Reject file collisions and incomplete deletions. Unsupported files still
/// receive patch validation, but have no analysis report.
fn apply_file_patch(
    worker: &mut analysis::Worker,
    parsed: &mut Vec<analysis::ParsedFile>,
    root: &Path,
    patch: &input::FilePatch,
) -> Result<PatchChange> {
    let old_path = patch.old_path.as_ref().map(|path| root.join(path));
    let new_path = patch.new_path.as_ref().map(|path| root.join(path));
    let old_target = old_path
        .as_ref()
        .map(|path| input::path_identity(path))
        .transpose()?;
    let new_target = new_path
        .as_ref()
        .map(|path| input::path_identity(path))
        .transpose()?;
    let destination_exists = if let Some(path) = &new_path {
        match std::fs::symlink_metadata(path) {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        }
    } else {
        false
    };
    if let Some(new_path) = &new_path
        && old_path.as_ref() != Some(new_path)
        && (destination_exists
            || parsed
                .iter()
                .any(|file| Some(&file.facts.target) == new_target.as_ref()))
    {
        return Err(format!(
            "The patch would overwrite another file: {}",
            new_path.display()
        )
        .into());
    }
    let old_index = old_target
        .as_ref()
        .and_then(|target| parsed.iter().position(|file| file.facts.target == *target));
    let source: std::sync::Arc<[u8]> = match (old_index, &old_path) {
        (Some(index), _) => std::sync::Arc::clone(&parsed[index].facts.source),
        (None, Some(path)) => std::fs::read(path)?.into(),
        (None, None) => Vec::new().into(),
    };
    let applied = input::apply_patch(&source, patch)?;
    let new_file = match &new_path {
        Some(path) => {
            let analysis_path = old_index
                .filter(|_| old_path.as_ref() == Some(path))
                .map_or(path, |index| &parsed[index].facts.path);
            match worker.select_file(analysis_path)? {
                Some(choice) => Some(worker.analyze_selected_source(
                    analysis_path,
                    applied.source,
                    old_index.map(|index| &parsed[index]),
                    &applied.edits,
                    choice,
                )?),
                None => None,
            }
        }
        None if !applied.source.is_empty() => {
            return Err("A deletion patch must remove the complete file".into());
        }
        _ => None,
    };
    if let Some(index) = old_index {
        parsed.remove(index);
    }
    if let Some(file) = new_file {
        parsed.push(file);
    }
    Ok(PatchChange {
        old_path,
        new_path,
        old_target,
        new_target,
        edits: applied.edits,
    })
}

/// Compare function scores and map old decision positions through source edits.
fn function_deltas(
    before: Option<&model::FileAnalysis>,
    after: Option<&model::FileAnalysis>,
    edits: &[tree_sitter::InputEdit],
) -> Vec<model::FunctionDelta> {
    let old_functions = before.map_or(&[][..], |file| file.functions.as_slice());
    let new_functions = after.map_or(&[][..], |file| file.functions.as_slice());
    let mut available: Vec<_> = new_functions.iter().map(Some).collect();
    let mut pairs = Vec::new();
    for old in old_functions {
        let mapped = edited_position(old.start_byte, edits);
        let matches: Vec<_> = available
            .iter()
            .enumerate()
            .filter_map(|(index, new)| {
                new.filter(|new| new.name == old.name)
                    .map(|new| (index, new))
            })
            .collect();
        let index = matches
            .iter()
            .find(|(_, new)| mapped == Some(new.start_byte))
            .map(|(index, _)| *index)
            .or_else(|| {
                (matches.len() == 1
                    && old_functions
                        .iter()
                        .filter(|function| function.name == old.name)
                        .count()
                        == 1)
                    .then(|| matches[0].0)
            });
        pairs.push((Some(old), index.and_then(|index| available[index].take())));
    }
    pairs.extend(available.into_iter().flatten().map(|new| (None, Some(new))));
    pairs
        .into_iter()
        .map(|(old, new)| {
            let old_nloc = old.map_or(0, |function| function.nloc);
            let new_nloc = new.map_or(0, |function| function.nloc);
            let old_cc = old.map_or(0, |function| function.cyclomatic_complexity);
            let new_cc = new.map_or(0, |function| function.cyclomatic_complexity);
            let old_density = old.map_or(0.0, |function| function.cyclomatic_density);
            let new_density = new.map_or(0.0, |function| function.cyclomatic_density);
            let (added_contributions, removed_contributions) =
                contribution_changes(old, new, edits);
            model::FunctionDelta {
                name: old
                    .or(new)
                    .expect("A function pair has a function")
                    .name
                    .clone(),
                nloc: model::MetricDelta {
                    before: old_nloc,
                    after: new_nloc,
                    delta: new_nloc as i64 - old_nloc as i64,
                },
                cyclomatic_complexity: model::MetricDelta {
                    before: old_cc,
                    after: new_cc,
                    delta: new_cc as i64 - old_cc as i64,
                },
                cyclomatic_density: model::MetricDelta {
                    before: old_density,
                    after: new_density,
                    delta: new_density - old_density,
                },
                added_contributions,
                removed_contributions,
            }
        })
        .collect()
}

/// Compare decisions by role and edited start position; retain matched baselines.
fn contribution_changes(
    before: Option<&model::FunctionAnalysis>,
    after: Option<&model::FunctionAnalysis>,
    edits: &[tree_sitter::InputEdit],
) -> (
    Vec<model::ComplexityContribution>,
    Vec<model::ComplexityContribution>,
) {
    let mut added = after.map_or_else(Vec::new, |function| function.contributions.clone());
    let mut removed = Vec::new();
    for old in before
        .into_iter()
        .flat_map(|function| &function.contributions)
    {
        let mapped = edited_position(old.start_byte, edits);
        let index = added.iter().position(|new| {
            new.kind == old.kind
                && new.value == old.value
                && (old.kind == "baseline" || mapped == Some(new.start_byte))
        });
        if let Some(index) = index {
            added.remove(index);
        } else {
            let mut contribution = old.clone();
            contribution.value = -contribution.value;
            removed.push(contribution);
        }
    }
    (added, removed)
}

/// Map a byte position through ordered edits; return none for replaced bytes.
fn edited_position(position: usize, edits: &[tree_sitter::InputEdit]) -> Option<usize> {
    edits.iter().try_fold(position, |position, edit| {
        if position < edit.start_byte {
            Some(position)
        } else if position >= edit.old_end_byte {
            position
                .checked_sub(edit.old_end_byte - edit.start_byte)?
                .checked_add(edit.new_end_byte - edit.start_byte)
        } else {
            None
        }
    })
}
