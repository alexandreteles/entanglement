//! Read static Composer data through the candidate's file snapshot.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::graph::FileFactsView;
use crate::snapshot::Snapshot;

use super::super::files::indexing::normalize;

pub(super) struct Projects {
    pub roots: Vec<PathBuf>,
    manifests: BTreeMap<PathBuf, Manifest>,
    dependencies: BTreeMap<PathBuf, BTreeMap<String, BTreeSet<String>>>,
    requirements: BTreeMap<PathBuf, BTreeSet<String>>,
    names: BTreeMap<PathBuf, String>,
    external: Vec<(PathBuf, Value)>,
}

enum Manifest {
    Missing,
    Invalid,
    Valid(Value),
}

impl Projects {
    pub fn new(facts: &[FileFactsView<'_>], snapshot: &Snapshot) -> Self {
        let paths = facts
            .iter()
            .map(|fact| fact.target.as_path())
            .collect::<Vec<_>>();
        let common = paths.first().map_or_else(PathBuf::new, |first| {
            first
                .ancestors()
                .skip(1)
                .find(|root| paths.iter().all(|path| path.starts_with(root)))
                .unwrap_or(Path::new(""))
                .to_path_buf()
        });
        let roots = paths
            .iter()
            .map(|path| {
                path.ancestors()
                    .skip(1)
                    .find(|root| !matches!(snapshot.read(&root.join("composer.json")), Ok(None)))
                    .map_or_else(|| common.clone(), Path::to_path_buf)
            })
            .collect::<Vec<_>>();
        let manifests: BTreeMap<_, _> = roots
            .iter()
            .map(|root| {
                let manifest = match snapshot.read(&root.join("composer.json")) {
                    Ok(None) => Manifest::Missing,
                    Ok(Some(bytes)) => {
                        serde_json::from_slice(&bytes).map_or(Manifest::Invalid, Manifest::Valid)
                    }
                    Err(_) => Manifest::Invalid,
                };
                (root.clone(), manifest)
            })
            .collect();
        let mut projects = Self {
            roots,
            manifests,
            dependencies: BTreeMap::new(),
            requirements: BTreeMap::new(),
            names: BTreeMap::new(),
            external: Vec::new(),
        };
        let configs = projects
            .manifests
            .iter()
            .filter_map(|(root, config)| match config {
                Manifest::Valid(config) => Some((root.clone(), config.clone())),
                _ => None,
            })
            .collect::<Vec<_>>();
        for (root, config) in &configs {
            projects.read_project(root, config, snapshot);
        }
        for (caller, _) in &configs {
            for (target, config) in &configs {
                let Some(name) = projects.names.get(target) else {
                    continue;
                };
                if projects.selected_root(caller, name) == Some(target) {
                    projects
                        .dependencies
                        .entry(caller.clone())
                        .or_default()
                        .entry(name.clone())
                        .or_insert_with(|| requirements(config, false));
                }
            }
        }
        projects
    }

    /// Read one project's root requirements and locked package metadata.
    fn read_project(&mut self, root: &Path, config: &Value, snapshot: &Snapshot) {
        if let Some(name) = config.get("name").and_then(Value::as_str) {
            self.names.insert(root.to_path_buf(), name.to_owned());
        }
        self.requirements
            .insert(root.to_path_buf(), requirements(config, true));
        let packages = ["composer.lock", "vendor/composer/installed.json"]
            .into_iter()
            .filter_map(|file| read_json(snapshot, &root.join(file)))
            .flat_map(package_metadata);
        let graph = self.dependencies.entry(root.to_path_buf()).or_default();
        for package in packages {
            if let Some(name) = package.get("name").and_then(Value::as_str) {
                // Prefer the caller's lock entry to installed or local metadata.
                graph
                    .entry(name.to_owned())
                    .or_insert_with(|| requirements(&package, false));
                self.external.push((root.to_path_buf(), package));
            }
        }
    }

    /// Select one local package, using path metadata to distinguish duplicate names.
    fn selected_root(&self, caller: &Path, name: &str) -> Option<&PathBuf> {
        let candidates = self
            .names
            .iter()
            .filter(|(_, candidate)| candidate.as_str() == name)
            .map(|(root, _)| root)
            .collect::<Vec<_>>();
        let locked = self
            .external
            .iter()
            .filter(|(root, package)| {
                root == caller && package.get("name").and_then(Value::as_str) == Some(name)
            })
            .filter_map(|(_, package)| {
                let dist = package.get("dist")?;
                (dist.get("type")?.as_str()? == "path").then_some(dist.get("url")?.as_str()?)
            })
            .collect::<Vec<_>>();
        if !locked.is_empty() {
            return unique_root(&candidates, caller, &locked);
        }
        let repositories = match self.manifests.get(caller) {
            Some(Manifest::Valid(config)) => config.get("repositories").and_then(Value::as_array),
            _ => None,
        };
        let paths = repositories
            .into_iter()
            .flatten()
            .filter_map(|repository| {
                (repository.get("type")?.as_str()? == "path")
                    .then_some(repository.get("url")?.as_str()?)
            })
            .collect::<Vec<_>>();
        unique_root(&candidates, caller, &paths)
            .or_else(|| (candidates.len() == 1).then(|| candidates[0]))
    }

    /// Keep dependency graphs local to the caller's Composer project.
    pub fn reaches(&self, caller: usize, target: usize) -> bool {
        let (caller, target) = (&self.roots[caller], &self.roots[target]);
        if caller == target {
            return true;
        }
        let Some(name) = self.names.get(target) else {
            return false;
        };
        if self.selected_root(caller, name) != Some(target) {
            return false;
        }
        let mut seen = BTreeSet::new();
        let mut pending = self
            .requirements
            .get(caller)
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect::<Vec<_>>();
        while let Some(dependency) = pending.pop() {
            if dependency == name {
                return true;
            }
            if seen.insert(dependency) {
                pending.extend(
                    self.dependencies
                        .get(caller)
                        .and_then(|graph| graph.get(dependency))
                        .into_iter()
                        .flatten()
                        .map(String::as_str),
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
        let Some(config) = self.manifests.get(root) else {
            return false;
        };
        let config = match config {
            Manifest::Missing => return true,
            Manifest::Invalid => return false,
            Manifest::Valid(config) => config,
        };
        ["autoload", "autoload-dev"]
            .into_iter()
            .filter(|key| *key != "autoload-dev" || self.roots[caller] == *root)
            .filter_map(|key| config.get(key))
            .any(|autoload| autoload_matches(root, autoload, path, name, class))
    }

    /// A locked package can prove that a namespace belongs to external code.
    pub fn external(&self, file: usize, name: &str) -> bool {
        self.external
            .iter()
            .filter(|(root, _)| *root == self.roots[file])
            .any(|(_, package)| {
                package.get("autoload").is_some_and(|autoload| {
                    ["psr-4", "psr-0"]
                        .into_iter()
                        .filter_map(|key| autoload.get(key)?.as_object())
                        .flat_map(|mapping| mapping.keys())
                        .any(|prefix| !prefix.is_empty() && name.starts_with(prefix))
                })
            })
    }
}

/// Read root development requirements only when this is the starting project.
fn requirements(package: &Value, development: bool) -> BTreeSet<String> {
    ["require", "require-dev"]
        .into_iter()
        .filter(|key| development || *key == "require")
        .filter_map(|key| package.get(key)?.as_object())
        .flat_map(|requires| requires.keys())
        .filter(|name| name.contains('/'))
        .cloned()
        .collect()
}

/// Match exact local paths; ambiguous and wildcard repository paths stay unknown.
fn unique_root<'a>(
    candidates: &[&'a PathBuf],
    caller: &Path,
    paths: &[&str],
) -> Option<&'a PathBuf> {
    let matches = candidates
        .iter()
        .copied()
        .filter(|candidate| {
            paths
                .iter()
                .filter(|path| !path.contains('*'))
                .any(|path| normalize(candidate) == normalize(&caller.join(path)))
        })
        .collect::<Vec<_>>();
    (matches.len() == 1).then(|| matches[0])
}

fn autoload_matches(root: &Path, autoload: &Value, path: &Path, name: &str, class: bool) -> bool {
    if paths(autoload.get("files")).any(|entry| normalize(&root.join(entry)) == normalize(path)) {
        return true;
    }
    if !class {
        return false;
    }
    (paths(autoload.get("classmap")).any(|entry| path_matches(root, entry, path))
        && !paths(autoload.get("exclude-from-classmap"))
            .any(|pattern| path_matches(root, pattern, path)))
        || ["psr-4", "psr-0"].into_iter().any(|kind| {
            autoload
                .get(kind)
                .and_then(Value::as_object)
                .is_some_and(|mappings| {
                    mappings.iter().any(|(prefix, directories)| {
                        let Some(suffix) = name.strip_prefix(prefix) else {
                            return false;
                        };
                        let relative = if kind == "psr-0" {
                            let (namespace, class) = name.rsplit_once('\\').unwrap_or(("", name));
                            [namespace.replace('\\', "/"), class.replace('_', "/")]
                                .into_iter()
                                .filter(|part| !part.is_empty())
                                .collect::<Vec<_>>()
                                .join("/")
                        } else {
                            suffix.replace('\\', "/")
                        };
                        paths(Some(directories)).any(|directory| {
                            normalize(&root.join(directory).join(format!("{relative}.php")))
                                == normalize(path)
                        })
                    })
                })
        })
}

fn paths(value: Option<&Value>) -> impl Iterator<Item = &str> {
    value.into_iter().flat_map(|value| {
        value.as_str().into_iter().chain(
            value
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str),
        )
    })
}

fn path_matches(root: &Path, pattern: &str, path: &Path) -> bool {
    let pattern = pattern.trim_start_matches('/');
    if !pattern.contains('*') {
        return normalize(path).starts_with(normalize(&root.join(pattern)));
    }
    let mut builder = ignore::gitignore::GitignoreBuilder::new(root);
    builder.add_line(None, pattern).is_ok()
        && builder
            .build()
            .is_ok_and(|matcher| matcher.matched(path, false).is_ignore())
}

fn read_json(snapshot: &Snapshot, path: &Path) -> Option<Value> {
    serde_json::from_slice(&snapshot.read(path).ok()??).ok()
}

/// Accept Composer lock files and both installed-package JSON formats.
fn package_metadata(value: Value) -> Vec<Value> {
    if let Some(packages) = value.as_array() {
        return packages.clone();
    }
    ["packages", "packages-dev"]
        .into_iter()
        .filter_map(|key| value.get(key)?.as_array())
        .flatten()
        .cloned()
        .collect()
}
