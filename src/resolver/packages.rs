//! Resolve Go package members without inferring types or choosing build tags.

use super::analysis;
use crate::graph::FileFactsView;

use std::collections::BTreeMap;
use std::path::PathBuf;

use rayon::prelude::*;

use crate::graph::ProgramGraph;
use crate::languages::ResolutionFamily;
use crate::model::{Reference, ReferenceAnalysis, Resolution};
use crate::snapshot::Snapshot;

use super::files::indexing::normalize;

mod contexts;
mod imports;
mod inventory;
mod members;
mod modules;
mod universe;

use contexts::{Context, PackageKey};
use imports::ImportTarget;
use members::Package;
use modules::ModuleState;

pub(super) fn resolve(
    facts: &[FileFactsView<'_>],
    snapshot: &Snapshot,
    graph: &ProgramGraph<'_>,
) -> Vec<Option<Vec<ReferenceAnalysis>>> {
    let index = Index::new(snapshot, graph);
    facts
        .par_iter()
        .enumerate()
        .map(|(file_index, fact)| {
            is_go(fact).then(|| {
                fact.references()
                    .map(|reference| {
                        analysis(
                            reference,
                            index.resolve_reference(file_index, fact, reference, graph),
                            None,
                        )
                    })
                    .collect()
            })
        })
        .collect()
}

struct Index {
    packages: BTreeMap<PackageKey, Package>,
    modules: BTreeMap<PathBuf, ModuleState>,
}

impl Index {
    fn new(snapshot: &Snapshot, graph: &ProgramGraph<'_>) -> Self {
        let packages = members::index(graph);
        let modules = packages
            .keys()
            .map(|key| {
                (
                    key.directory.clone(),
                    modules::nearest(&key.directory, snapshot),
                )
            })
            .collect();
        Self { packages, modules }
    }

    fn resolve_reference(
        &self,
        file_index: usize,
        fact: &FileFactsView<'_>,
        reference: &Reference,
        graph: &ProgramGraph<'_>,
    ) -> Resolution {
        let Some(name) = reference.path.first() else {
            return Resolution::Unresolved;
        };
        if contexts::is_local(graph, file_index, reference, name) {
            return Resolution::Unresolved;
        }
        members::agree(
            contexts::for_reference(fact, graph, file_index, reference.context_id)
                .iter()
                .map(|context| self.resolve_in_context(fact, reference, context, name)),
        )
    }

    fn resolve_in_context(
        &self,
        fact: &FileFactsView<'_>,
        reference: &Reference,
        context: &Context,
        name: &str,
    ) -> Resolution {
        if let Some(own) = self.own_members(context, name) {
            return if reference.path.len() == 1 {
                own
            } else {
                Resolution::Unresolved
            };
        }
        let mut bound = self.bound_imports(fact, reference.context_id, context, name);
        match (bound.next(), bound.next(), reference.path.as_slice()) {
            (Some(ImportTarget::Packages(packages)), None, [_, member]) => {
                self.imported_members(&packages, member)
            }
            (Some(ImportTarget::External), None, [_, _]) => Resolution::External,
            (None, _, [_]) if universe::UNIVERSE.contains(&name) => Resolution::External,
            _ => Resolution::Unresolved,
        }
    }
}

fn is_go(fact: &FileFactsView<'_>) -> bool {
    fact.resolution_family == ResolutionFamily::GoPackages
}

/// Keep package identity and build uncertainty for structural method checks.
pub(super) fn structural_contexts(graph: &ProgramGraph<'_>) -> BTreeMap<(usize, usize), String> {
    graph
        .files()
        .iter()
        .enumerate()
        .filter(|(_, fact)| is_go(fact))
        .flat_map(|(file, fact)| {
            fact.semantic()
                .map(|store| store.semantic.context_id)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .filter_map(|context| {
                    let contexts = contexts::for_reference(fact, graph, file, context);
                    let keys = contexts
                        .iter()
                        .map(|context| &context.key)
                        .collect::<std::collections::BTreeSet<_>>();
                    (keys.len() == 1 && contexts.iter().all(|context| !context.conditional))
                        .then(|| ((file, context), format!("{:?}", keys.first().unwrap())))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}
