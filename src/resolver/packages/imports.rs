use crate::model::{FileFacts, Import};

use super::{Context, Index, PackageKey, modules};

pub(super) enum ImportTarget {
    Packages(Vec<PackageKey>),
    External,
    Unresolved,
}

impl Index {
    pub(super) fn bound_imports<'a>(
        &'a self,
        fact: &'a FileFacts,
        id: usize,
        context: &'a Context,
        name: &'a str,
    ) -> impl Iterator<Item = ImportTarget> + 'a {
        fact.imports
            .iter()
            .filter(move |import| import.context_id == id)
            .filter_map(move |import| {
                let target = self.import_target(context, import);
                binding_names(import, &target)
                    .iter()
                    .any(|bound| bound == name)
                    .then_some(target)
            })
    }

    fn import_target(&self, context: &Context, import: &Import) -> ImportTarget {
        let Some(source) = import
            .source
            .as_deref()
            .filter(|source| modules::valid_path(source))
        else {
            return ImportTarget::Unresolved;
        };
        let Some(Ok(module)) = self.modules.get(&context.key.directory) else {
            return ImportTarget::Unresolved;
        };
        let Some(relative) = module
            .as_ref()
            .and_then(|module| modules::relative(module, source))
        else {
            return ImportTarget::External;
        };
        // A directory nested under another go.mod is not in the importer's module.
        if self.modules.get(&relative) != Some(&Ok(module.clone())) {
            return ImportTarget::Unresolved;
        }
        let packages = self
            .packages
            .iter()
            .filter(|(key, package)| {
                key.directory == relative && key.injection.is_none() && package.production
            })
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        if packages.is_empty() {
            ImportTarget::Unresolved
        } else {
            ImportTarget::Packages(packages)
        }
    }
}

/// An unavailable unnamed external package has only a heuristic path-derived binding.
/// This can classify references as external, but never establish an exact symbol.
fn binding_names(import: &Import, target: &ImportTarget) -> Vec<String> {
    match (import.alias.as_deref(), target) {
        (Some("_" | "."), _) => Vec::new(),
        (Some(alias), _) => vec![alias.to_owned()],
        (None, ImportTarget::Packages(packages)) => {
            packages.iter().map(|key| key.name.clone()).collect()
        }
        (None, _) => inferred_name(import).into_iter().collect(),
    }
}

fn inferred_name(import: &Import) -> Option<String> {
    let mut elements = import.path.iter().rev();
    let last = elements.next().filter(|element| !is_major_version(element));
    last.or_else(|| elements.next()).cloned()
}

fn is_major_version(element: &str) -> bool {
    element
        .strip_prefix('v')
        .and_then(|digits| digits.parse::<u32>().ok())
        .is_some_and(|major| major >= 2)
}
