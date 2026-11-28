//! Source membership and build symbols for each C# compilation.
mod conditions;
mod framework;
mod items;
mod manifest;
mod views;

use crate::snapshot::Snapshot;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub(crate) struct View {
    pub project: PathBuf,
    pub configuration: String,
    pub framework: String,
    pub namespace: String,
    pub symbols: BTreeSet<String>,
    pub references: Vec<PathBuf>,
    pub usings: Vec<(String, String, bool)>,
    pub limits: BTreeSet<String>,
    default_items: bool,
    default_content: bool,
    razor_components: bool,
    razor_views: bool,
    excludes: globset::GlobSet,
    rules: Vec<items::CompileRule>,
}

impl View {
    fn includes(&self, source: &Path) -> bool {
        let relative = source
            .strip_prefix(self.project.parent().unwrap_or(&self.project))
            .ok();
        let default = !self.excludes.is_match(source)
            && relative.is_some_and(|p| {
                !p.components()
                    .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
            });
        let (item, enabled) = if extension(source, "razor") {
            ("RazorComponent", self.razor_components)
        } else if extension(source, "cshtml") {
            ("RazorGenerate", self.razor_views)
        } else {
            return self.item(
                "Compile",
                source,
                default && self.default_items && extension(source, "cs"),
            );
        };
        let content = enabled && self.item("Content", source, default && self.default_content);
        self.item(item, source, content)
    }

    fn item(&self, name: &str, source: &Path, default: bool) -> bool {
        self.rules
            .iter()
            .rfind(|r| r.item == name && r.applies(source))
            .map_or(default, |r| r.include)
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/csharp_msbuild.rs"]
mod msbuild_tests;

#[derive(Clone, Debug, Default)]
pub(crate) struct Project {
    pub masks: BTreeMap<PathBuf, BTreeMap<usize, super::conditional::Mask>>,
    pub views: Vec<View>,
    pub files: BTreeMap<PathBuf, Vec<usize>>,
    pub reachable: Vec<BTreeSet<usize>>,
}

impl Project {
    pub(crate) fn load(root: &Path, files: &[PathBuf], snapshot: &Snapshot) -> crate::Result<Self> {
        let paths = snapshot.paths_with_extension(root, "csproj")?;
        let views = paths
            .into_iter()
            .filter(|p| snapshot.read(p).ok().flatten().is_some())
            .flat_map(|p| views::read(&p, snapshot))
            .collect::<Vec<_>>();
        let membership = files
            .iter()
            .map(|file| {
                (
                    file.clone(),
                    views
                        .iter()
                        .enumerate()
                        .filter(|(_, v)| v.includes(file))
                        .map(|(i, _)| i)
                        .collect(),
                )
            })
            .collect();
        let reachable = (0..views.len())
            .map(|index| reachable(&views, index))
            .collect();
        Ok(Self {
            views,
            files: membership,
            reachable,
            ..Default::default()
        })
    }

    pub fn owners(&self, path: &Path) -> &[usize] {
        self.files.get(path).map_or(&[], Vec::as_slice)
    }

    pub(crate) fn include_aliases(&mut self, path: &Path, aliases: &[PathBuf]) {
        if let Some(owners) = self.files.get_mut(path) {
            owners.extend(
                self.views
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| aliases.iter().any(|alias| v.includes(alias)))
                    .map(|(index, _)| index),
            );
            owners.sort_unstable();
            owners.dedup();
        }
    }

    pub fn active(&self, path: &Path, view: usize, position: usize) -> bool {
        self.masks
            .get(path)
            .and_then(|views| views.get(&view))
            .is_none_or(|mask| mask.active(position))
    }
}

fn reachable(views: &[View], first: usize) -> BTreeSet<usize> {
    let mut result = BTreeSet::new();
    let mut pending = vec![first];
    while let Some(index) = pending.pop() {
        if !result.insert(index) {
            continue;
        }
        let view = &views[index];
        for path in &view.references {
            let options: Vec<_> = views
                .iter()
                .enumerate()
                .filter(|(_, v)| &v.project == path)
                .collect();
            let config = if options
                .iter()
                .any(|(_, v)| v.configuration == view.configuration)
            {
                view.configuration.as_str()
            } else {
                "Debug"
            };
            let choices = options
                .into_iter()
                .filter(|(_, v)| v.configuration == config)
                .collect::<Vec<_>>();
            pending.extend(framework::nearest(&view.framework, &choices));
        }
    }
    result
}

pub(crate) fn extension(path: &Path, suffix: &str) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case(suffix))
}

/// Find solution siblings for file candidates and the nearest standalone project otherwise.
pub(crate) fn root(source: &Path) -> Option<&Path> {
    let mut project = None;
    for dir in source.ancestors().skip(1) {
        let mut solution = false;
        for entry in dir.read_dir().ok()?.flatten() {
            let p = entry.path();
            if extension(&p, "csproj") {
                project.get_or_insert(dir);
            }
            solution |= ["sln", "slnx", "slnf"].iter().any(|e| extension(&p, e));
        }
        if solution || dir.join(".git").exists() {
            return Some(dir);
        }
    }
    project
}
