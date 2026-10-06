pub(super) mod dotted;

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::languages::FileModuleRules;

use super::Target;
use super::indexing::{Index, module_candidates, normalize};

pub(super) fn resolve_slash(
    index: &Index<'_>,
    file: usize,
    source: &str,
    rules: &FileModuleRules,
) -> Vec<Target> {
    if is_unresolved_alias(source, rules) {
        return vec![Target::Unresolved];
    }
    if !is_relative_slash(source) {
        return vec![Target::External];
    }
    let requests = index.facts[file]
        .aliases
        .iter()
        .filter_map(|alias| alias.parent().map(|parent| parent.join(source)));
    resolve_requests(index, requests, rules)
}

pub(super) fn resolve_requests(
    index: &Index<'_>,
    requests: impl IntoIterator<Item = PathBuf>,
    rules: &FileModuleRules,
) -> Vec<Target> {
    let files = requests
        .into_iter()
        .flat_map(|path| module_candidates(&path, rules))
        .filter_map(|path| index.by_path.get(&normalize(&path)))
        .flatten()
        .copied()
        .filter(|file| {
            index.facts[*file]
                .file_module_rules
                .is_some_and(|target| target.layout == rules.layout)
        })
        .collect::<BTreeSet<_>>();
    if files.is_empty() {
        vec![Target::Unresolved]
    } else {
        files.into_iter().map(Target::Namespace).collect()
    }
}

pub(super) fn resolve_submodule(index: &Index<'_>, package_file: usize, name: &str) -> Vec<Target> {
    let fact = &index.facts[package_file];
    let Some(rules) = fact.file_module_rules else {
        return Vec::new();
    };
    if !matches!(
        rules.layout,
        crate::languages::FileModuleLayout::Dotted {
            submodule_imports: true,
            ..
        }
    ) {
        return Vec::new();
    }
    let requests = fact
        .aliases
        .iter()
        .filter(|path| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| rules.index_stems.contains(&stem))
        })
        .filter_map(|path| path.parent())
        .filter_map(|parent| dotted::module_path(name).map(|module| parent.join(module)))
        .collect::<BTreeSet<_>>();
    resolve_requests(index, requests, rules)
}

fn is_relative_slash(source: &str) -> bool {
    matches!(source, "." | "..") || source.starts_with("./") || source.starts_with("../")
}

fn is_unresolved_alias(source: &str, rules: &FileModuleRules) -> bool {
    rules
        .unresolved_prefixes
        .iter()
        .any(|prefix| source.starts_with(prefix))
        || source.starts_with('/')
}
