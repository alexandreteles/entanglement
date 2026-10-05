//! Parse source files and derive language-independent metrics.

mod facts;
mod injections;
mod worker;

pub(crate) use worker::{ParsedFile, Worker};

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use rayon::prelude::*;

use crate::Result;
use crate::input;

/// Analyze each supported target once and keep its logical paths as aliases.
pub(crate) fn analyze_paths<I, T: Send>(
    paths: I,
    project: impl Fn(ParsedFile) -> T + Sync,
) -> Result<Vec<T>>
where
    I: IntoIterator<Item = Result<PathBuf>>,
    I::IntoIter: Send,
{
    let mut groups = BTreeMap::<PathBuf, BTreeSet<PathBuf>>::new();
    for path in paths {
        let path = path?;
        groups
            .entry(input::path_identity(&path)?)
            .or_default()
            .insert(path);
    }

    let parsed: Vec<_> = groups
        .into_iter()
        .par_bridge()
        .map_init(
            || Worker::new().map_err(|error| error.to_string()),
            |worker, (target, aliases)| match worker {
                Ok(worker) => worker
                    .analyze_target(target, aliases.into_iter().collect())
                    .map(|file| file.map(|file| (file.facts.path.clone(), project(file)))),
                Err(error) => Err(error.clone().into()),
            },
        )
        .collect::<Result<Vec<_>>>()?;
    let mut files: Vec<_> = parsed.into_iter().flatten().collect();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files.into_iter().map(|(_, file)| file).collect())
}
