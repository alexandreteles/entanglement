//! Resolve Node subpath imports, such as SvelteKit's `#lib/*`, through the
//! `imports` field of the nearest `package.json`.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::Deserialize;

use crate::languages::FileModuleRules;

use super::super::Target;
use super::super::indexing::Index;
use super::resolve_requests;

/// The conditions TypeScript applies to ESM imports, in Node's matching order.
const CONDITIONS: &[&str] = &["types", "import", "default"];

#[derive(Deserialize)]
struct PackageManifest {
    #[serde(default)]
    imports: IndexMap<String, ImportTarget>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ImportTarget {
    Path(String),
    Conditions(IndexMap<String, ImportTarget>),
    Other(serde::de::IgnoredAny),
}

pub(in crate::resolver::files) fn resolve(
    index: &Index<'_>,
    file: usize,
    source: &str,
    rules: &FileModuleRules,
) -> Vec<Target> {
    let mut requests = Vec::new();
    for alias in &index.facts[file].aliases {
        let Some((scope, target)) = package_target(alias, source) else {
            return vec![Target::Unresolved];
        };
        match target.strip_prefix("./") {
            Some(relative) => requests.push(scope.join(relative)),
            None => return vec![Target::External],
        }
    }
    resolve_requests(index, requests, rules)
}

/// Node reads only the nearest `package.json`, prefers an exact key, and
/// otherwise uses the `*` pattern with the longest prefix.
fn package_target(file: &Path, specifier: &str) -> Option<(PathBuf, String)> {
    let scope = file
        .ancestors()
        .skip(1)
        .find(|directory| directory.join("package.json").is_file())?;
    let manifest = std::fs::read_to_string(scope.join("package.json")).ok()?;
    let imports = serde_json::from_str::<PackageManifest>(&manifest)
        .ok()?
        .imports;
    let target = match imports.get(specifier).filter(|_| !specifier.contains('*')) {
        Some(target) => target.path()?.to_owned(),
        None => {
            let (key, matched) = imports
                .keys()
                .filter_map(|key| pattern_match(key, specifier).map(|matched| (key, matched)))
                .max_by_key(|(key, _)| (key.find('*'), key.len()))?;
            imports[key].path()?.replace('*', matched)
        }
    };
    Some((scope.to_path_buf(), target))
}

fn pattern_match<'s>(key: &str, specifier: &'s str) -> Option<&'s str> {
    let (prefix, suffix) = key.split_once('*')?;
    if suffix.contains('*') || specifier.len() < key.len() {
        return None;
    }
    specifier.strip_prefix(prefix)?.strip_suffix(suffix)
}

impl ImportTarget {
    fn path(&self) -> Option<&str> {
        match self {
            Self::Path(path) => Some(path),
            Self::Conditions(conditions) => conditions
                .iter()
                .find(|(condition, _)| CONDITIONS.contains(&condition.as_str()))
                .and_then(|(_, target)| target.path()),
            Self::Other(_) => None,
        }
    }
}
