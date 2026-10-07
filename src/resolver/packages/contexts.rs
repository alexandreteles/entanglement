use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::model::{DefinitionKind, FileFacts, Reference};

use super::normalize;

/// Native package files share members; separate injected programs never do.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct PackageKey {
    pub directory: PathBuf,
    pub name: String,
    pub injection: Option<(PathBuf, usize)>,
}

pub(super) struct Context {
    pub key: PackageKey,
    pub test: bool,
    pub conditional: bool,
}

pub(super) fn for_reference(fact: &FileFacts, context_id: usize) -> Vec<Context> {
    let names = fact
        .definitions
        .iter()
        .filter(|item| item.kind == DefinitionKind::Module && item.context_id == context_id)
        .map(|item| item.name.as_str())
        .collect::<BTreeSet<_>>();
    if names.len() != 1 {
        return Vec::new();
    }
    let name = *names.first().expect("one package");
    fact.aliases
        .iter()
        .filter_map(|alias| {
            let directory = normalize(alias.parent()?);
            Some(Context {
                key: PackageKey {
                    directory,
                    name: name.to_owned(),
                    injection: (fact.analysis.language != "go")
                        .then(|| (fact.target.clone(), context_id)),
                },
                test: is_test(alias),
                conditional: super::inventory::conditional(alias, fact),
            })
        })
        .collect()
}

pub(super) fn is_test(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with("_test.go"))
}

pub(super) fn is_local(fact: &FileFacts, reference: &Reference, name: &str) -> bool {
    fact.locals.iter().any(|local| {
        local.context_id == reference.context_id
            && local.name == name
            && local.scope_start <= reference.start_byte
            && reference.start_byte < local.scope_end
    })
}
