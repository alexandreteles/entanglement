mod changes;
mod patch;
mod patch_paths;
mod paths;

use crate::metrics::selection::Selection;

/// Run one file, repository, patch, or candidate command.
pub(crate) fn run(
    command: crate::cli::Command,
    selection: Selection,
) -> crate::Result<crate::model::AnalysisResult> {
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
            Ok(result(vec![parsed.facts], selection))
        }
        crate::cli::Command::Repo { path } => {
            let path = std::path::absolute(path)?;
            let facts = crate::analysis::analyze_paths(
                crate::input::discover(&path)?,
                selection,
                |file| file.facts,
            )?;
            Ok(result(facts, selection))
        }
        crate::cli::Command::Patch { file, diff } => patch::analyze(&file, &diff, true, selection),
        crate::cli::Command::Candidate { path, diff } => {
            patch::analyze(&path, &diff, false, selection)
        }
    }
}

fn result(
    mut facts: Vec<crate::model::FileFacts>,
    selection: Selection,
) -> crate::model::AnalysisResult {
    crate::resolver::resolve(&mut facts, selection);
    crate::model::AnalysisResult {
        files: facts.into_iter().map(|fact| fact.analysis).collect(),
        patch: None,
        maintainability_index_bands: crate::metrics::maintainability::BANDS,
    }
}
