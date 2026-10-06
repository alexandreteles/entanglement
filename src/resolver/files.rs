//! Resolve imports between files using language-neutral file-module facts.

use rayon::prelude::*;

use crate::model::{FileFacts, ReferenceAnalysis, Resolution};

mod bindings;
mod exports;
mod indexing;
mod references;
mod symbols;

use indexing::Index;

/// Build results for file-module languages without exposing another syntax parser.
pub(super) fn resolve(facts: &[FileFacts]) -> Vec<Option<Vec<ReferenceAnalysis>>> {
    let index = Index::new(facts);
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
