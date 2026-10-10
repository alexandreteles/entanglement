use std::path::Path;

use tree_sitter::InputEdit;

use crate::metrics::selection::Selection;
use crate::{analysis, input, model, resolver, snapshot};

use super::changes::PatchChange;
use super::patch_paths::PatchPaths;
use super::paths;

/// Apply a diff in memory and resolve the full before and after file sets.
///
/// A file target accepts one patch whose old path matches that file. A
/// directory target accepts patches relative to that directory. Reject empty
/// diffs and attempts to overwrite another file. Never write source bytes.
pub(super) fn analyze(
    path: &Path,
    diff: &Path,
    single: bool,
    selection: Selection,
) -> crate::Result<model::AnalysisResult> {
    let diff = input::read_diff(diff)?;
    let patches = input::parse_diff(&diff)?;
    let (root, source_paths) = paths::patch_sources(path, &patches, single)?;
    let mut parsed = analysis::analyze_paths(source_paths, selection, std::convert::identity)?;
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
        .collect::<crate::Result<_>>()?;
    let mut after: Vec<_> = parsed.into_iter().map(|file| file.facts).collect();
    after.sort_by(|left, right| left.path.cmp(&right.path));
    resolver::resolve_with_snapshot(&mut after, selection, &after_snapshot);
    let files = changes
        .into_iter()
        .map(|change| change.compare(&before, &after, selection))
        .collect();
    Ok(model::AnalysisResult {
        files: after.into_iter().map(|file| file.analysis).collect(),
        patch: Some(model::PatchAnalysis { files }),
        maintainability_index_bands: crate::metrics::maintainability::BANDS,
    })
}

fn apply_file_patch(
    worker: &mut analysis::Worker,
    parsed: &mut Vec<analysis::ParsedFile>,
    root: &Path,
    patch: &input::FilePatch,
    source_snapshot: &snapshot::Snapshot,
) -> crate::Result<PatchChange> {
    let paths = PatchPaths::new(root, patch)?;
    paths.ensure_destination_available(parsed)?;
    let old_index = paths.old_index(parsed);
    let source = paths.source(parsed, old_index)?;
    let applied = source_snapshot.apply_patch(
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
        paths,
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
) -> crate::Result<Option<analysis::ParsedFile>> {
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
