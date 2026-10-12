use std::collections::{BTreeMap, BTreeSet};

use crate::graph::ProgramGraph;
use crate::model::{DefinitionKind, Resolution, SymbolId};

use super::{Context, Index, PackageKey, contexts, is_go};

pub(super) struct Member {
    symbol: SymbolId,
    public: bool,
    test: bool,
    conditional: bool,
}

#[derive(Default)]
pub(super) struct Package {
    pub production: bool,
    members: BTreeMap<String, Vec<Member>>,
}

pub(super) fn index(graph: &ProgramGraph<'_>) -> BTreeMap<PackageKey, Package> {
    let mut packages = BTreeMap::<PackageKey, Package>::new();
    for file_index in 0..graph.file_count() {
        let fact = graph.file(file_index);
        if !is_go(fact) {
            continue;
        }
        let ids = graph
            .symbols_for_file(file_index)
            .map(|item| item.scope.context_id)
            .collect::<BTreeSet<_>>();
        for id in ids {
            for context in contexts::for_reference(fact, graph, file_index, id) {
                let package = packages.entry(context.key.clone()).or_default();
                package.production |= !context.test;
                add_members(package, graph, file_index, id, &context);
            }
        }
    }
    packages
}

fn add_members(
    package: &mut Package,
    graph: &ProgramGraph<'_>,
    file_index: usize,
    context_id: usize,
    context: &Context,
) {
    for symbol in graph.symbols_for_file(file_index).filter(|item| {
        item.scope.context_id == context_id
            && !matches!(item.kind, DefinitionKind::Method | DefinitionKind::Module)
    }) {
        package
            .members
            .entry(symbol.name.clone())
            .or_default()
            .push(Member {
                symbol: symbol.semantic_id.clone(),
                public: symbol.is_public,
                test: context.test,
                conditional: context.conditional,
            });
    }
}

impl Index {
    pub(super) fn own_members(&self, context: &Context, name: &str) -> Option<Resolution> {
        let members = self.packages.get(&context.key)?.members.get(name)?;
        select(members.iter().filter(|member| context.test || !member.test))
    }

    pub(super) fn imported_members(&self, packages: &[PackageKey], name: &str) -> Resolution {
        // Missing members in one possible package must not disappear from the outcome.
        agree(packages.iter().map(|key| {
            let members = self
                .packages
                .get(key)
                .and_then(|package| package.members.get(name));
            select(
                members
                    .into_iter()
                    .flatten()
                    .filter(|member| member.public && !member.test),
            )
            .unwrap_or(Resolution::Unresolved)
        }))
    }
}

fn select<'a>(members: impl Iterator<Item = &'a Member>) -> Option<Resolution> {
    let members = members.collect::<Vec<_>>();
    if members.is_empty() {
        return None;
    }
    let symbols = members
        .iter()
        .map(|item| item.symbol.clone())
        .collect::<BTreeSet<_>>();
    if symbols.len() == 1 && members.iter().any(|item| item.conditional) {
        return Some(Resolution::Unresolved);
    }
    Some(symbol_resolution(symbols))
}

/// Combine full context outcomes, never just their successful symbol matches.
pub(super) fn agree(outcomes: impl IntoIterator<Item = Resolution>) -> Resolution {
    let mut symbols = BTreeSet::new();
    let mut external = false;
    for outcome in outcomes {
        match outcome {
            Resolution::Exact(symbol) => {
                symbols.insert(symbol);
            }
            Resolution::Ambiguous(items) => symbols.extend(items),
            Resolution::External => external = true,
            Resolution::Unresolved => return Resolution::Unresolved,
        }
    }
    if external {
        return if symbols.is_empty() {
            Resolution::External
        } else {
            Resolution::Unresolved
        };
    }
    symbol_resolution(symbols)
}

fn symbol_resolution(symbols: BTreeSet<SymbolId>) -> Resolution {
    match symbols.len() {
        0 => Resolution::Unresolved,
        1 => Resolution::Exact(symbols.into_iter().next().expect("one symbol")),
        _ => Resolution::Ambiguous(symbols.into_iter().collect()),
    }
}
