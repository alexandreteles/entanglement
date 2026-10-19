//! Read static Composer data through the candidate's file snapshot.

use super::super::files::indexing::normalize;
use crate::{graph::FileFactsView, snapshot::Snapshot};
mod autoload;
use super::super::only;
use autoload::{autoload_matches, psr_routes};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub(super) struct Projects {
    pub roots: Vec<PathBuf>,
    projects: BTreeMap<PathBuf, Project>,
}

struct Project {
    manifest: Manifest,
    packages: BTreeMap<String, Vec<Value>>,
    external_prefixes: BTreeSet<String>,
}

enum Manifest {
    Missing,
    Invalid,
    Valid(Value),
}

impl Manifest {
    fn config(&self) -> &Value {
        match self {
            Self::Valid(config) => config,
            _ => &Value::Null,
        }
    }
}

impl Projects {
    pub fn new(facts: &[FileFactsView<'_>], snapshot: &Snapshot) -> Self {
        let paths = facts
            .iter()
            .map(|fact| fact.target.as_path())
            .collect::<Vec<_>>();
        let common = paths
            .first()
            .and_then(|first| {
                first
                    .ancestors()
                    .skip(1)
                    .find(|root| paths.iter().all(|path| path.starts_with(root)))
            })
            .unwrap_or(Path::new(""))
            .to_path_buf();
        let mut manifests = BTreeMap::new();
        let roots = paths
            .iter()
            .map(|path| {
                path.ancestors()
                    .skip(1)
                    .find(|root| {
                        !matches!(
                            manifests.entry(root.to_path_buf()).or_insert_with(|| {
                                match snapshot.read(&root.join("composer.json")) {
                                    Ok(None) => Manifest::Missing,
                                    Ok(Some(bytes)) => serde_json::from_slice(&bytes)
                                        .map_or(Manifest::Invalid, Manifest::Valid),
                                    Err(_) => Manifest::Invalid,
                                }
                            }),
                            Manifest::Missing
                        )
                    })
                    .unwrap_or(&common)
                    .to_path_buf()
            })
            .collect::<Vec<_>>();
        let projects = roots
            .iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|root| {
                let manifest = manifests.remove(root).unwrap_or(Manifest::Missing);
                (root.clone(), Project::new(root, manifest, snapshot))
            })
            .collect();
        Self { roots, projects }
    }

    /// Select one local package, using path metadata to distinguish duplicate names.
    fn selected_root(&self, caller: &Path, name: &str) -> Option<&PathBuf> {
        let candidates = self
            .projects
            .iter()
            .filter(|(_, project)| project.manifest.config()["name"].as_str() == Some(name))
            .map(|(root, _)| root)
            .collect::<Vec<_>>();
        let project = &self.projects[caller];
        let locked = project
            .packages
            .get(name)
            .into_iter()
            .flatten()
            .filter_map(|package| path_url(&package["dist"]))
            .collect::<Vec<_>>();
        if !locked.is_empty() {
            return unique_root(&candidates, caller, &locked);
        }
        let paths = project.manifest.config()["repositories"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(path_url)
            .collect::<Vec<_>>();
        unique_root(&candidates, caller, &paths).or_else(|| only(candidates))
    }

    /// Keep dependency graphs local to the caller's Composer project.
    pub fn reaches(&self, caller: usize, target: usize) -> bool {
        let (caller, target) = (&self.roots[caller], &self.roots[target]);
        if caller == target {
            return true;
        }
        let project = &self.projects[caller];
        let Some(name) = self.projects[target].manifest.config()["name"].as_str() else {
            return false;
        };
        if self.selected_root(caller, name) != Some(target) {
            return false;
        }
        let mut seen = BTreeSet::new();
        let mut pending = requirements(project.manifest.config(), true).collect::<Vec<_>>();
        while let Some(dependency) = pending.pop() {
            if dependency == name {
                return true;
            }
            if seen.insert(dependency) {
                let package = project
                    .packages
                    .get(dependency)
                    .and_then(|packages| packages.first())
                    .or_else(|| {
                        self.selected_root(caller, dependency)
                            .map(|root| self.projects[root].manifest.config())
                    });
                pending.extend(
                    package
                        .into_iter()
                        .flat_map(|package| requirements(package, false)),
                );
            }
        }
        false
    }

    /// Return whether a source file has a declared autoload route.
    pub fn autoloads(
        &self,
        caller: usize,
        target: usize,
        path: &Path,
        name: &str,
        class: bool,
    ) -> bool {
        if caller == target {
            return true;
        }
        let root = &self.roots[target];
        match &self.projects[root].manifest {
            Manifest::Missing => true,
            Manifest::Valid(config) => ["autoload", "autoload-dev"]
                .into_iter()
                .filter(|key| *key != "autoload-dev" || self.roots[caller] == *root)
                .filter_map(|key| config.get(key))
                .any(|autoload| autoload_matches(root, autoload, path, name, class)),
            _ => false,
        }
    }

    /// A locked package can prove that a namespace belongs to external code.
    pub fn external(&self, file: usize, name: &str) -> bool {
        self.projects.get(&self.roots[file]).is_some_and(|project| {
            project
                .external_prefixes
                .iter()
                .any(|prefix| name.starts_with(prefix))
        })
    }
}

impl Project {
    /// Keep each package record once and cache external namespace prefixes.
    fn new(root: &Path, manifest: Manifest, snapshot: &Snapshot) -> Self {
        let mut packages: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        let mut external_prefixes = BTreeSet::new();
        for package in ["composer.lock", "vendor/composer/installed.json"]
            .into_iter()
            .filter(|_| matches!(manifest, Manifest::Valid(_)))
            .filter_map(|file| read_json(snapshot, &root.join(file)))
            .flat_map(package_metadata)
        {
            let Some(name) = package.get("name").and_then(Value::as_str) else {
                continue;
            };
            external_prefixes.extend(
                psr_routes(&package["autoload"])
                    .filter(|(_, prefix, _)| !prefix.is_empty())
                    .map(|(_, prefix, _)| prefix.to_owned()),
            );
            packages.entry(name.to_owned()).or_default().push(package);
        }
        Self {
            manifest,
            packages,
            external_prefixes,
        }
    }
}

fn path_url(value: &Value) -> Option<&str> {
    (value.get("type")?.as_str()? == "path").then_some(value.get("url")?.as_str()?)
}

/// Read root development requirements only when this is the starting project.
fn requirements(package: &Value, development: bool) -> impl Iterator<Item = &str> {
    ["require", "require-dev"]
        .into_iter()
        .filter(move |key| development || *key == "require")
        .filter_map(move |key| package.get(key)?.as_object())
        .flat_map(|requires| requires.keys())
        .filter(|name| name.contains('/'))
        .map(String::as_str)
}

/// Match exact local paths; ambiguous and wildcard repository paths stay unknown.
fn unique_root<'a>(
    candidates: &[&'a PathBuf],
    caller: &Path,
    paths: &[&str],
) -> Option<&'a PathBuf> {
    let matches = candidates.iter().copied().filter(|candidate| {
        paths
            .iter()
            .filter(|path| !path.contains('*'))
            .any(|path| normalize(candidate) == normalize(&caller.join(path)))
    });
    only(matches)
}

fn read_json(snapshot: &Snapshot, path: &Path) -> Option<Value> {
    serde_json::from_slice(&snapshot.read(path).ok()??).ok()
}

/// Accept Composer lock files and both installed-package JSON formats without copying packages.
fn package_metadata(value: Value) -> Vec<Value> {
    match value {
        Value::Array(packages) => packages,
        Value::Object(mut object) => ["packages", "packages-dev"]
            .into_iter()
            .flat_map(|key| match object.remove(key) {
                Some(Value::Array(packages)) => packages,
                _ => Vec::new(),
            })
            .collect(),
        _ => Vec::new(),
    }
}
