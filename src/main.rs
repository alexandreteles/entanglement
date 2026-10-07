mod analysis;
mod cli;
mod input;
mod languages;
mod metrics;
mod model;
mod report_delta;
mod resolver;
mod snapshot;
mod ui;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use metrics::selection::Selection;

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
    let selection = Selection::new(&arguments.metrics);
    let report = run(arguments.command, selection).and_then(|result| match arguments.format {
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

/// Execute a file or repository analysis, or apply a virtual patch.
fn run(command: cli::Command, selection: Selection) -> Result<model::AnalysisResult> {
    match command {
        cli::Command::File { path } => {
            let path = std::path::absolute(path)?;
            let parsed = analysis::Worker::new(selection)?
                .analyze_path(&path)?
                .ok_or_else(|| {
                    std::io::Error::other(format!(
                        "No registered grammar supports {}",
                        path.display()
                    ))
                })?;
            let mut facts = vec![parsed.facts];
            resolver::resolve(&mut facts, selection);
            Ok(model::AnalysisResult {
                files: facts.into_iter().map(|fact| fact.analysis).collect(),
                patch: None,
                maintainability_index_bands: metrics::maintainability::BANDS,
            })
        }
        cli::Command::Repo { path } => {
            let path = std::path::absolute(path)?;
            let mut facts: Vec<_> =
                analysis::analyze_paths(input::discover(&path)?, selection, |file| file.facts)?;
            resolver::resolve(&mut facts, selection);
            Ok(model::AnalysisResult {
                files: facts.into_iter().map(|fact| fact.analysis).collect(),
                patch: None,
                maintainability_index_bands: metrics::maintainability::BANDS,
            })
        }
        cli::Command::Patch { file, diff } => analyze_patch(&file, &diff, true, selection),
        cli::Command::Candidate { path, diff } => analyze_patch(&path, &diff, false, selection),
    }
}

/// Apply a diff in memory and resolve the full before and after file sets.
///
/// A file target accepts one patch whose old path matches that file. A
/// directory target accepts patches relative to that directory. Reject empty
/// diffs and attempts to overwrite another file. Never write source bytes.
fn analyze_patch(
    path: &Path,
    diff: &Path,
    single: bool,
    selection: Selection,
) -> Result<model::AnalysisResult> {
    let diff = input::read_diff(diff)?;
    let patches = input::parse_diff(&diff)?;
    let (root, paths) = patch_sources(path, &patches, single)?;
    let mut parsed = analysis::analyze_paths(paths, selection, std::convert::identity)?;
    if single && parsed.is_empty() {
        return Err("The selected file has no supported language".into());
    }
    let mut before: Vec<_> = parsed.iter().map(|file| file.facts.clone()).collect();
    let before_snapshot = snapshot::Snapshot::default();
    resolver::resolve_with_snapshot(&mut before, selection, &before_snapshot);
    let after_snapshot = before_snapshot.clone();
    let mut worker = analysis::Worker::new(selection)?;
    let changes: Vec<_> = patches
        .iter()
        .map(|patch| apply_file_patch(&mut worker, &mut parsed, &root, patch, &after_snapshot))
        .collect::<Result<_>>()?;
    let mut after: Vec<_> = parsed.into_iter().map(|file| file.facts).collect();
    after.sort_by(|a, b| a.path.cmp(&b.path));
    resolver::resolve_with_snapshot(&mut after, selection, &after_snapshot);
    let files = changes
        .into_iter()
        .map(|change| change.compare(&before, &after, selection))
        .collect();
    Ok(model::AnalysisResult {
        files: after.into_iter().map(|file| file.analysis).collect(),
        patch: Some(model::PatchAnalysis { files }),
        maintainability_index_bands: metrics::maintainability::BANDS,
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
            let manifests = languages::Registry::shared()?.project_manifests();
            target
                .ancestors()
                .skip(1)
                .find(|directory| is_project_root(directory, &manifests))
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

/// Recognize a project root using manifests supplied by language descriptors.
fn is_project_root(directory: &Path, manifests: &[&str]) -> bool {
    directory.join(".git").exists()
        || manifests
            .iter()
            .any(|manifest| directory.join(manifest).is_file())
}

/// The paths and incremental edits for one virtual file change.
struct PatchChange {
    old_path: Option<PathBuf>,
    new_path: Option<PathBuf>,
    old_target: Option<PathBuf>,
    new_target: Option<PathBuf>,
    edits: Vec<InputEdit>,
}

struct PatchPaths {
    old_path: Option<PathBuf>,
    new_path: Option<PathBuf>,
    old_target: Option<PathBuf>,
    new_target: Option<PathBuf>,
}

impl PatchPaths {
    fn new(root: &Path, patch: &input::FilePatch<'_>) -> Result<Self> {
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
        Ok(Self {
            old_path,
            new_path,
            old_target,
            new_target,
        })
    }

    fn ensure_destination_available(&self, parsed: &[analysis::ParsedFile]) -> Result<()> {
        let Some(new_path) = &self.new_path else {
            return Ok(());
        };
        if self.old_path.as_ref() == Some(new_path) {
            return Ok(());
        }
        let exists = match std::fs::symlink_metadata(new_path) {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        };
        if exists
            || parsed
                .iter()
                .any(|file| Some(&file.facts.target) == self.new_target.as_ref())
        {
            return Err(format!(
                "The patch would overwrite another file: {}",
                new_path.display()
            )
            .into());
        }
        Ok(())
    }

    fn old_index(&self, parsed: &[analysis::ParsedFile]) -> Option<usize> {
        self.old_target
            .as_ref()
            .and_then(|target| parsed.iter().position(|file| file.facts.target == *target))
    }

    fn source(
        &self,
        parsed: &[analysis::ParsedFile],
        old_index: Option<usize>,
    ) -> Result<Arc<[u8]>> {
        match (old_index, &self.old_path) {
            (Some(index), _) => Ok(Arc::clone(&parsed[index].facts.source)),
            (None, Some(path)) => Ok(std::fs::read(path)?.into()),
            (None, None) => Ok(Vec::new().into()),
        }
    }

    fn analysis_path<'a>(
        &'a self,
        parsed: &'a [analysis::ParsedFile],
        old_index: Option<usize>,
        destination: &'a Path,
    ) -> &'a Path {
        old_index
            .filter(|_| self.old_path == self.new_path)
            .map_or(destination, |index| parsed[index].facts.path.as_path())
    }
}

impl PatchChange {
    /// Compare the resolved reports and function contributions for this change.
    fn compare(
        self,
        before: &[model::FileFacts],
        after: &[model::FileFacts],
        selection: Selection,
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
            maintainability_index: report_delta::maintainability_delta(
                old.as_ref()
                    .and_then(|file| file.maintainability_index.as_ref()),
                new.as_ref()
                    .and_then(|file| file.maintainability_index.as_ref()),
            ),
            halstead: report_delta::halstead_delta(
                old.as_ref().map(|file| &file.halstead),
                new.as_ref().map(|file| &file.halstead),
                &self.edits,
                selection,
            ),
            functions: report_delta::function_deltas(
                old.as_ref(),
                new.as_ref(),
                &self.edits,
                selection,
            ),
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
    snapshot: &snapshot::Snapshot,
) -> Result<PatchChange> {
    let paths = PatchPaths::new(root, patch)?;
    paths.ensure_destination_available(parsed)?;
    let old_index = paths.old_index(parsed);
    let source = paths.source(parsed, old_index)?;
    let applied = snapshot.apply_patch(
        &source,
        patch,
        paths.old_path.as_deref(),
        paths.new_path.as_deref(),
    )?;
    let new_file = analyze_after_patch(
        worker,
        parsed,
        &paths,
        old_index,
        applied.source,
        &applied.edits,
    )?;
    replace_parsed_file(parsed, old_index, new_file);
    Ok(PatchChange {
        old_path: paths.old_path,
        new_path: paths.new_path,
        old_target: paths.old_target,
        new_target: paths.new_target,
        edits: applied.edits,
    })
}

fn analyze_after_patch(
    worker: &mut analysis::Worker,
    parsed: &[analysis::ParsedFile],
    paths: &PatchPaths,
    old_index: Option<usize>,
    source: Vec<u8>,
    edits: &[InputEdit],
) -> Result<Option<analysis::ParsedFile>> {
    let Some(path) = &paths.new_path else {
        if !source.is_empty() {
            return Err("A deletion patch must remove the complete file".into());
        }
        return Ok(None);
    };
    let analysis_path = paths.analysis_path(parsed, old_index, path);
    let Some(choice) = worker.select_file(analysis_path)? else {
        return Ok(None);
    };
    worker
        .analyze_selected_source(
            analysis_path,
            source,
            old_index.map(|index| &parsed[index]),
            edits,
            choice,
        )
        .map(Some)
}

fn replace_parsed_file(
    parsed: &mut Vec<analysis::ParsedFile>,
    old_index: Option<usize>,
    new_file: Option<analysis::ParsedFile>,
) {
    if let Some(index) = old_index {
        parsed.remove(index);
    }
    if let Some(file) = new_file {
        parsed.push(file);
    }
}
