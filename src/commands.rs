mod changes;
mod patch;
mod patch_paths;
mod paths;

use crate::metrics::selection::Selection;

/// Run one file, repository, patch, or candidate command.
pub(crate) fn run(
    command: crate::cli::Command,
    selection: Selection,
    database: Option<std::path::PathBuf>,
) -> crate::Result<crate::model::AnalysisResult> {
    let snapshot = crate::snapshot::Snapshot::new(database);
    match command {
        crate::cli::Command::File { path } => {
            let path = std::path::absolute(path)?;
            let parsed = crate::analysis::Worker::new(selection)?
                .analyze_path(&path)?
                .ok_or_else(|| {
                    std::io::Error::other(format!(
                        "No registered grammar supports {}",
                        path.display()
                    ))
                })?;
            result(vec![parsed], selection, "local", snapshot)
        }
        crate::cli::Command::Repo { path } => {
            let path = std::path::absolute(path)?;
            let facts = crate::analysis::analyze_paths(
                crate::input::discover(&path)?,
                selection,
                std::convert::identity,
            )?;
            result(facts, selection, "repo", snapshot)
        }
        crate::cli::Command::Patch { file, diff } => {
            patch::analyze(&file, &diff, true, selection, snapshot)
        }
        crate::cli::Command::Candidate { path, diff } => {
            patch::analyze(&path, &diff, false, selection, snapshot)
        }
    }
}

fn result(
    mut files: Vec<crate::analysis::ParsedFile>,
    selection: Selection,
    scope: &str,
    mut snapshot: crate::snapshot::Snapshot,
) -> crate::Result<crate::model::AnalysisResult> {
    crate::analysis::c_family::prepare(&mut files, selection, &mut snapshot)?;
    let mut facts: Vec<_> = files.into_iter().map(|file| file.facts).collect();
    crate::resolver::resolve_with_snapshot(&mut facts, selection, &snapshot);
    let structure = selection
        .includes(crate::metrics::selection::Metric::Structure)
        .then(|| crate::structure::analyze(&facts, scope, &snapshot));
    Ok(crate::model::AnalysisResult {
        structure,
        files: facts.into_iter().map(|fact| fact.analysis).collect(),
        patch: None,
        maintainability_index_bands: crate::metrics::maintainability::BANDS,
    })
}
