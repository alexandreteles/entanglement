use crate::graph::FileFactsView;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::graph::ProgramGraph;
use crate::model::{DefinitionKind, Reference};

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

pub(super) fn for_reference(
    fact: &FileFactsView<'_>,
    graph: &ProgramGraph<'_>,
    file_index: usize,
    context_id: usize,
) -> Vec<Context> {
    let names = graph
        .symbols_for_file(file_index)
        .filter(|item| item.kind == DefinitionKind::Module && item.scope.context_id == context_id)
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

pub(super) fn is_local(
    graph: &ProgramGraph<'_>,
    file_index: usize,
    reference: &Reference,
    name: &str,
) -> bool {
    graph.has_local_binding(file_index, name, reference.context_id, reference.start_byte)
}
