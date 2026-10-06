use std::path::{Path, PathBuf};

use crate::languages::FileModuleRules;

use super::super::Target;
use super::super::indexing::{Index, normalize};
use super::{is_unresolved_alias, resolve_requests};

pub(in crate::resolver::files) fn resolve(
    index: &Index<'_>,
    file: usize,
    source: &str,
    rules: &FileModuleRules,
    roots: &[&str],
    project_markers: &[&str],
) -> Vec<Target> {
    if is_unresolved_alias(source, rules) {
        return vec![Target::Unresolved];
    }
    match relative_module(source) {
        Some((levels, suffix)) => resolve_relative(index, file, levels, suffix, rules),
        None => resolve_absolute(index, file, source, roots, project_markers, rules),
    }
}

pub(super) fn module_path(module: &str) -> Option<PathBuf> {
    if module.is_empty() {
        return Some(PathBuf::new());
    }
    let mut path = PathBuf::new();
    for part in module.split('.') {
        if part.is_empty() || part == ".." || part.contains('/') || part.contains('\\') {
            return None;
        }
        path.push(part);
    }
    Some(path)
}

fn resolve_relative(
    index: &Index<'_>,
    file: usize,
    levels: usize,
    suffix: &str,
    rules: &FileModuleRules,
) -> Vec<Target> {
    let requests = index.facts[file]
        .aliases
        .iter()
        .filter_map(|alias| package_directory(index, alias, rules))
        .filter_map(|package| ascend_package(index, package, levels, rules))
        .filter_map(|package| module_path(suffix).map(|suffix| package.join(suffix)));
    resolve_requests(index, requests, rules)
}

fn package_directory(index: &Index<'_>, file: &Path, rules: &FileModuleRules) -> Option<PathBuf> {
    let parent = file.parent()?;
    if file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| rules.index_stems.contains(&stem))
        || is_package(index, parent, rules)
    {
        Some(parent.to_path_buf())
    } else {
        None
    }
}

fn ascend_package(
    index: &Index<'_>,
    mut package: PathBuf,
    levels: usize,
    rules: &FileModuleRules,
) -> Option<PathBuf> {
    for _ in 1..levels {
        package = package.parent()?.to_path_buf();
        if !is_package(index, &package, rules) {
            return None;
        }
    }
    Some(package)
}

fn is_package(index: &Index<'_>, path: &Path, rules: &FileModuleRules) -> bool {
    rules.index_stems.iter().any(|stem| {
        rules.extensions.iter().any(|extension| {
            let mut init = path.join(stem).into_os_string();
            init.push(".");
            init.push(extension);
            index.by_path.contains_key(&normalize(&PathBuf::from(init)))
        })
    })
}

fn resolve_absolute(
    index: &Index<'_>,
    file: usize,
    source: &str,
    roots: &[&str],
    project_markers: &[&str],
    rules: &FileModuleRules,
) -> Vec<Target> {
    let Some(module) = module_path(source) else {
        return vec![Target::Unresolved];
    };
    let Some(project_root) = index.facts[file]
        .aliases
        .iter()
        .find_map(|alias| project_root(alias, project_markers))
    else {
        return vec![Target::External];
    };
    let requests = roots
        .iter()
        .map(|root| project_root.join(root).join(&module))
        .collect::<Vec<_>>();
    let targets = resolve_requests(index, requests.clone(), rules);
    if targets != [Target::Unresolved] {
        return targets;
    }
    if local_top_module(index, &requests, &module, rules) {
        vec![Target::Unresolved]
    } else {
        vec![Target::External]
    }
}

fn project_root(path: &Path, markers: &[&str]) -> Option<PathBuf> {
    path.ancestors().find_map(|directory| {
        markers
            .iter()
            .any(|marker| directory.join(marker).is_file())
            .then(|| directory.to_path_buf())
    })
}

fn local_top_module(
    index: &Index<'_>,
    requests: &[PathBuf],
    module: &Path,
    rules: &FileModuleRules,
) -> bool {
    let Some(top) = module.components().next() else {
        return false;
    };
    requests.iter().any(|request| {
        let root = request.ancestors().nth(module.components().count());
        let Some(root) = root else { return false };
        let top_path = root.join(top.as_os_str());
        !resolve_requests(index, [top_path.clone()], rules)
            .iter()
            .all(|target| matches!(target, Target::Unresolved))
            || index
                .by_path
                .keys()
                .any(|candidate| candidate.starts_with(normalize(&top_path)))
    })
}

fn relative_module(source: &str) -> Option<(usize, &str)> {
    let levels = source.bytes().take_while(|byte| *byte == b'.').count();
    (levels > 0).then_some((levels, &source[levels..]))
}
