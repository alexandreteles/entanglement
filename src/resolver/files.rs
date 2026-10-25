//! Resolve imports between files using language-neutral file-module facts.

use crate::graph::FileFactsView;

use rayon::prelude::*;

use crate::graph::ProgramGraph;
use crate::model::{ReferenceAnalysis, Resolution};

mod bindings;
mod exports;
pub(super) mod indexing;
mod layout;
mod references;
mod symbols;

use indexing::Index;

/// Build results for file-module languages without exposing another syntax parser.
pub(super) fn resolve(
    facts: &[FileFactsView<'_>],
    graph: &ProgramGraph<'_>,
) -> Vec<Option<Vec<ReferenceAnalysis>>> {
    let index = Index::new(facts, graph);
    facts
        .par_iter()
        .enumerate()
        .map(|(file, fact)| index.resolve_file(file, fact))
        .collect()
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Target {
    Symbol(crate::model::SymbolId),
    Namespace(usize),
    External,
    Unresolved,
}

fn to_resolution(targets: Vec<Target>) -> Resolution {
    let targets = unique_targets(targets);
    if targets
        .iter()
        .any(|target| matches!(target, Target::Unresolved))
    {
        return Resolution::Unresolved;
    }
    match targets.as_slice() {
        [] => Resolution::Unresolved,
        [Target::Symbol(symbol)] => Resolution::Exact(symbol.clone()),
        [Target::External] => Resolution::External,
        _ if targets
            .iter()
            .all(|target| matches!(target, Target::Symbol(_))) =>
        {
            Resolution::Ambiguous(
                targets
                    .into_iter()
                    .filter_map(|target| match target {
                        Target::Symbol(symbol) => Some(symbol),
                        _ => None,
                    })
                    .collect(),
            )
        }
        _ => Resolution::Unresolved,
    }
}

fn unique_targets(mut targets: Vec<Target>) -> Vec<Target> {
    targets.sort();
    targets.dedup();
    targets
}

/// Read static resource targets without adding compiler or runtime assumptions.
pub(super) fn structural_resources(
    facts: &[FileFactsView<'_>],
    graph: &ProgramGraph<'_>,
    sources: Vec<(usize, usize, crate::structure::resources::ResourceFact)>,
) -> Vec<(
    usize,
    usize,
    crate::structure::resources::ResourceFact,
    Resolution,
)> {
    let index = Index::new(facts, graph);
    sources
        .into_iter()
        .map(|(file, context, resource)| {
            let targets = if resource.dynamic {
                vec![Target::Unresolved]
            } else {
                index.resolve_resource(file, &resource.source)
            };
            let resolution = match targets.as_slice() {
                [Target::Namespace(target)] => Resolution::Exact(crate::model::SymbolId {
                    file: facts[*target].analysis.path.clone(),
                    module: crate::model::ModulePath::default(),
                    name: resource.source.clone(),
                    kind: crate::model::DefinitionKind::Module,
                    start_byte: 0,
                }),
                [Target::External] => Resolution::External,
                _ => Resolution::Unresolved,
            };
            (file, context, resource, resolution)
        })
        .collect()
}

/// Read module targets with the same import rules used by reference resolution.
pub(super) fn structural_modules(
    facts: &[FileFactsView<'_>],
    graph: &ProgramGraph<'_>,
) -> Vec<(usize, usize, usize, String, Resolution)> {
    let index = Index::new(facts, graph);
    facts
        .iter()
        .enumerate()
        .flat_map(|(file, fact)| {
            let sources = fact
                .imports()
                .filter_map(|import| {
                    import
                        .source
                        .as_ref()
                        .map(|source| (import.start_byte, import.end_byte, source))
                })
                .chain(fact.exports().filter_map(|export| {
                    export
                        .source
                        .as_ref()
                        .map(|source| (export.start_byte, export.end_byte, source))
                }));
            sources
                .map(|(start, end, source)| {
                    let targets = index.resolve_module(file, source);
                    let resolution = match targets.as_slice() {
                        [Target::Namespace(target)] => Resolution::Exact(crate::model::SymbolId {
                            file: facts[*target].analysis.path.clone(),
                            module: crate::model::ModulePath::default(),
                            name: source.clone(),
                            kind: crate::model::DefinitionKind::Module,
                            start_byte: 0,
                        }),
                        [Target::External] => Resolution::External,
                        _ => Resolution::Unresolved,
                    };
                    (file, start, end, source.clone(), resolution)
                })
                .collect::<Vec<_>>()
        })
        .collect()
}
