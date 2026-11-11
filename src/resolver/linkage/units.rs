//! Build each file's translation unit from its project includes.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use super::is_linkage;
use crate::graph::{FileFactsView, ProgramGraph};
use crate::model::Import;
use crate::resolver::files::indexing::normalize;

pub(super) struct Unit {
    /// The file itself and every project file it includes, transitively.
    pub files: BTreeSet<usize>,
    /// Some include names a file outside the project, such as a system header.
    pub has_external_includes: bool,
}

pub(super) fn build(graph: &ProgramGraph<'_>) -> Vec<Unit> {
    let includes = Includes::new(graph);
    let direct = (0..graph.file_count())
        .map(|file| includes.of(graph.file(file)))
        .collect::<Vec<_>>();
    (0..graph.file_count())
        .map(|file| {
            let mut unit = Unit {
                files: BTreeSet::from([file]),
                has_external_includes: false,
            };
            let mut pending = vec![file];
            while let Some(current) = pending.pop() {
                for (_, targets) in &direct[current] {
                    unit.has_external_includes |= targets.is_empty();
                    pending.extend(targets.iter().filter(|target| unit.files.insert(**target)));
                }
            }
            unit
        })
        .collect()
}

/// Project C and C++ file paths, indexed by file name for include lookup.
pub(super) struct Includes(HashMap<String, Vec<(PathBuf, usize)>>);

impl Includes {
    pub(super) fn new(graph: &ProgramGraph<'_>) -> Self {
        let mut paths = HashMap::<String, Vec<(PathBuf, usize)>>::new();
        for file in (0..graph.file_count()).filter(|file| is_linkage(graph.file(*file))) {
            for alias in graph.file(file).aliases {
                let path = normalize(alias);
                if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                    paths.entry(name.to_owned()).or_default().push((path, file));
                }
            }
        }
        Self(paths)
    }

    /// Each include of a C or C++ file with the project files it may name.
    /// An include with no project file names a system or external header.
    pub(super) fn of<'a>(&self, fact: &FileFactsView<'a>) -> Vec<(&'a Import, BTreeSet<usize>)> {
        if !is_linkage(fact) {
            return Vec::new();
        }
        fact.imports()
            .filter_map(|import| Some((import, self.targets(fact, import.source.as_deref()?))))
            .collect()
    }

    /// A quoted include searches the including file's directory first. Otherwise
    /// the include matches project files whose path ends with it, as an include
    /// directory would provide.
    fn targets(&self, fact: &FileFactsView<'_>, source: &str) -> BTreeSet<usize> {
        let system = source.starts_with('<');
        let include = Path::new(source.trim_start_matches('<').trim_end_matches('>'));
        let Some(candidates) = include
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| self.0.get(name))
        else {
            return BTreeSet::new();
        };
        let beside = (!system)
            .then(|| fact.aliases.iter().filter_map(|alias| alias.parent()))
            .into_iter()
            .flatten()
            .map(|directory| normalize(&directory.join(include)))
            .collect::<Vec<_>>();
        let matching = |accept: &dyn Fn(&Path) -> bool| {
            candidates
                .iter()
                .filter(|(path, _)| accept(path))
                .map(|(_, target)| *target)
                .collect::<BTreeSet<_>>()
        };
        let found = matching(&|path| beside.iter().any(|candidate| candidate == path));
        if found.is_empty() {
            matching(&|path| path.ends_with(include))
        } else {
            found
        }
    }
}
