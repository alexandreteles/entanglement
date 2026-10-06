use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::model::{FileFacts, Import};

use super::{Index, directories, normalize};

pub(super) struct Module {
    root: PathBuf,
    path: String,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ImportTarget {
    Packages(Vec<(PathBuf, String)>),
    External,
    Unresolved,
}

impl Index {
    /// Return the targets of the file's imports that bind `name`.
    pub(super) fn bound_imports<'a>(
        &'a self,
        fact: &'a FileFacts,
        name: &'a str,
    ) -> impl Iterator<Item = ImportTarget> + 'a {
        fact.imports.iter().filter_map(move |import| {
            let target = self.import_target(fact, import);
            binding_names(import, &target)
                .iter()
                .any(|bound| bound == name)
                .then_some(target)
        })
    }

    /// Use the target only when every path that names the file agrees on it.
    fn import_target(&self, fact: &FileFacts, import: &Import) -> ImportTarget {
        let Some(source) = import.source.as_deref() else {
            return ImportTarget::Unresolved;
        };
        let targets = directories(fact)
            .map(|directory| self.directory_target(&directory, source))
            .collect::<BTreeSet<_>>();
        match targets.len() {
            1 => targets.into_iter().next().expect("One target is present"),
            _ => ImportTarget::Unresolved,
        }
    }

    fn directory_target(&self, directory: &Path, source: &str) -> ImportTarget {
        let Some(relative) = self
            .modules
            .get(directory)
            .and_then(Option::as_ref)
            .and_then(|module| module_relative(module, source))
        else {
            return ImportTarget::External;
        };
        let packages = self
            .packages
            .get(&relative)
            .into_iter()
            .flat_map(|packages| packages.keys())
            .filter(|package| !package.ends_with("_test"))
            .map(|package| (relative.clone(), package.clone()))
            .collect::<Vec<_>>();
        if packages.is_empty() {
            ImportTarget::Unresolved
        } else {
            ImportTarget::Packages(packages)
        }
    }
}

pub(super) fn nearest_module(directory: &Path) -> Option<Module> {
    directory.ancestors().find_map(|root| {
        let manifest = std::fs::read_to_string(root.join("go.mod")).ok()?;
        Some(Module {
            root: root.to_path_buf(),
            path: manifest.lines().find_map(module_directive)?,
        })
    })
}

fn module_directive(line: &str) -> Option<String> {
    let value = line.trim().strip_prefix("module")?;
    let value = value.split("//").next().unwrap_or_default();
    value
        .starts_with(char::is_whitespace)
        .then(|| value.trim().trim_matches(['"', '`']).to_owned())
}

/// Map an import path inside the module to its package directory.
fn module_relative(module: &Module, source: &str) -> Option<PathBuf> {
    let relative = match source.strip_prefix(&module.path)? {
        "" => "",
        rest => rest.strip_prefix('/')?,
    };
    Some(normalize(&module.root.join(relative)))
}

/// An unnamed import binds the imported package's declared name. Without a
/// local package, assume the last path element, skipping a `/vN` major
/// version suffix, which semantic import versioning starts at `v2`.
fn binding_names(import: &Import, target: &ImportTarget) -> Vec<String> {
    match (import.alias.as_deref(), target) {
        (Some("_" | "."), _) => Vec::new(),
        (Some(alias), _) => vec![alias.to_owned()],
        (None, ImportTarget::Packages(packages)) => packages
            .iter()
            .map(|(_, package)| package.clone())
            .collect(),
        (None, _) => {
            let mut elements = import.path.iter().rev();
            let last = elements.next().filter(|element| !is_major_version(element));
            last.or_else(|| elements.next())
                .cloned()
                .into_iter()
                .collect()
        }
    }
}

fn is_major_version(element: &str) -> bool {
    element
        .strip_prefix('v')
        .and_then(|digits| digits.parse::<u32>().ok())
        .is_some_and(|major| major >= 2)
}
