mod changes;
mod patch;
mod patch_paths;
mod paths;

use crate::metrics::selection::Selection;
use rayon::iter::{Either, IntoParallelIterator, ParallelIterator};

/// Run one file, repository, patch, or candidate command.
pub(crate) fn run(
    command: crate::cli::Command,
    selection: Selection,
    database: Option<std::path::PathBuf>,
) -> crate::Result<crate::model::AnalysisResult> {
    let mut snapshot = crate::snapshot::Snapshot::new(database);
    match command {
        crate::cli::Command::File { path } => {
            let path = std::path::absolute(path)?;
            if crate::languages::csharp::is_source(&path) {
                snapshot.csharp_root = crate::languages::csharp::project::root(&path)
                    .or(path.parent())
                    .map(Into::into);
            }
            let parsed = crate::analysis::Worker::new(selection)?
                .analyze_path(&path)?
                .ok_or_else(|| {
                    std::io::Error::other(format!(
                        "No registered grammar supports {}",
                        path.display()
                    ))
                })?;
            result(vec![parsed], Vec::new(), selection, "local", snapshot)
        }
        crate::cli::Command::Repo { path } => {
            let path = std::path::absolute(path)?;
            snapshot.csharp_root = Some(path.clone());
            let files = crate::analysis::analyze_paths(
                crate::input::discover(&path)?,
                selection,
                |parsed| {
                    if matches!(parsed.language.as_str(), "source.c" | "source.cpp")
                        || parsed.facts.contexts.iter().any(|s| {
                            s.semantic.csharp.as_ref().is_some_and(|data| {
                                data.directives.iter().any(|line| line.command == "if")
                            })
                        })
                    {
                        Either::Left(parsed)
                    } else {
                        Either::Right(parsed.facts)
                    }
                },
            )?;
            let (parsed, facts) = files.into_par_iter().partition_map(std::convert::identity);
            result(parsed, facts, selection, "repo", snapshot)
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
    mut facts: Vec<crate::model::FileFacts>,
    selection: Selection,
    scope: &str,
    mut snapshot: crate::snapshot::Snapshot,
) -> crate::Result<crate::model::AnalysisResult> {
    crate::analysis::prepare(&mut files, &facts, selection, &mut snapshot)?;
    facts.extend(files.into_iter().map(|file| file.facts));
    facts.sort_by(|left, right| left.path.cmp(&right.path));
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
