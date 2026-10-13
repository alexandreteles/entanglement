use crate::graph::FileFactsView;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use crate::graph::ProgramGraph;
use crate::languages::{FileModuleLayout, FileModuleRules, ResolutionFamily};

use super::Target;
use super::layout;

pub(super) struct Index<'a, 'g> {
    pub(super) facts: &'a [FileFactsView<'a>],
    pub(super) graph: &'g ProgramGraph<'a>,
    pub(super) by_path: BTreeMap<PathBuf, BTreeSet<usize>>,
}

impl<'a, 'g> Index<'a, 'g> {
    pub(super) fn new(facts: &'a [FileFactsView<'a>], graph: &'g ProgramGraph<'a>) -> Self {
        let mut by_path = BTreeMap::<PathBuf, BTreeSet<usize>>::new();
        for (path, indexes) in graph.aliases() {
            for index in indexes {
                if facts[*index].resolution_family == ResolutionFamily::FileModules {
                    by_path.entry(normalize(path)).or_default().insert(*index);
                }
            }
        }
        Self {
            facts,
            graph,
            by_path,
        }
    }

    pub(super) fn resolve_module(&self, file: usize, source: &str) -> Vec<Target> {
        let Some(rules) = self.facts[file].file_module_rules else {
            return vec![Target::Unresolved];
        };
        match rules.layout {
            FileModuleLayout::SlashRelative => layout::resolve_slash(self, file, source, rules),
            FileModuleLayout::Dotted {
                roots,
                project_markers,
                ..
            } => layout::dotted::resolve(self, file, source, rules, roots, project_markers),
        }
    }
}

pub(super) fn module_candidates(requested: &Path, rules: &FileModuleRules) -> Vec<PathBuf> {
    let extension = requested.extension().and_then(|value| value.to_str());
    let recognized = extension.is_some_and(|extension| rules.extensions.contains(&extension));
    let mut candidates = BTreeSet::from([requested.to_path_buf()]);
    if let Some(extension) = extension.filter(|_| recognized) {
        for mapped in remapped_extensions(extension, rules) {
            candidates.insert(requested.with_extension(mapped));
        }
    } else {
        for candidate in extension_candidates(requested, rules) {
            candidates.insert(candidate);
        }
    }
    candidates.into_iter().collect()
}

fn extension_candidates(requested: &Path, rules: &FileModuleRules) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for extension in rules.extensions {
        candidates.push(append_extension(requested, extension));
        for stem in rules.index_stems {
            candidates.push(append_extension(&requested.join(stem), extension));
        }
    }
    candidates
}

fn append_extension(path: &Path, extension: &str) -> PathBuf {
    let mut candidate = path.as_os_str().to_owned();
    candidate.push(".");
    candidate.push(extension);
    candidate.into()
}

fn remapped_extensions<'a>(extension: &str, rules: &'a FileModuleRules) -> &'a [&'static str] {
    rules
        .remaps
        .iter()
        .find_map(|(source, targets)| (*source == extension).then_some(*targets))
        .unwrap_or(&[])
}

pub(in crate::resolver) fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir if normalized.file_name().is_some() => {
                normalized.pop();
            }
            Component::Prefix(_)
            | Component::RootDir
            | Component::Normal(_)
            | Component::ParentDir => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}
