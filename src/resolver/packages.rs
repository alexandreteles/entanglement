//! Resolve Go package members without inferring types or choosing build tags.

use std::collections::BTreeMap;
use std::path::PathBuf;

use rayon::prelude::*;

use crate::graph::ProgramGraph;
use crate::languages::ResolutionFamily;
use crate::model::{FileFacts, Reference, ReferenceAnalysis, Resolution};
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
    facts: &[FileFacts],
    snapshot: &Snapshot,
    graph: &ProgramGraph<'_>,
) -> Vec<Option<Vec<ReferenceAnalysis>>> {
    let index = Index::new(snapshot, graph);
    facts
        .par_iter()
        .enumerate()
        .map(|(file_index, fact)| {
            is_go(fact).then(|| {
                fact.references
                    .iter()
                    .map(|reference| ReferenceAnalysis {
                        path: reference.path.clone(),
                        start_byte: reference.start_byte,
                        end_byte: reference.end_byte,
                        resolution: index.resolve_reference(file_index, fact, reference, graph),
                        dispatch: None,
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
        fact: &FileFacts,
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
        fact: &FileFacts,
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

fn is_go(fact: &FileFacts) -> bool {
    fact.resolution_family == ResolutionFamily::GoPackages
}
