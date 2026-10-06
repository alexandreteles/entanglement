use crate::model::{Definition, DefinitionKind, Export, FileFacts, SymbolId};

use super::{Target, unique_targets};

pub(super) fn local_symbols(
    fact: &FileFacts,
    name: &str,
    position: usize,
    context_id: usize,
) -> Option<(usize, Vec<Target>)> {
    let candidates = fact
        .definitions
        .iter()
        .filter(|definition| {
            definition.name == name
                && definition.kind != DefinitionKind::Method
                && definition.scope_start <= position
                && position < definition.scope_end
                && definition.context_id == context_id
        })
        .collect::<Vec<_>>();
    let rank = candidates
        .iter()
        .map(|definition| definition.scope_end.saturating_sub(definition.scope_start))
        .min()?;
    let targets = candidates
        .into_iter()
        .filter(|definition| definition.scope_end.saturating_sub(definition.scope_start) == rank)
        .map(|definition| symbol_target(fact, definition))
        .collect();
    Some((rank, unique_targets(targets)))
}

pub(super) fn anonymous_export_symbols(fact: &FileFacts, export: &Export) -> Vec<Target> {
    let definitions = fact
        .definitions
        .iter()
        .filter(|definition| {
            definition.kind != DefinitionKind::Method
                && definition.context_id == export.context_id
                && export.start_byte <= definition.start_byte
                && definition.end_byte <= export.end_byte
        })
        .map(|definition| symbol_target(fact, definition))
        .collect();
    unique_targets(definitions)
}

fn symbol_target(fact: &FileFacts, definition: &Definition) -> Target {
    Target::Symbol(SymbolId {
        file: fact.analysis.path.clone(),
        module: definition.module.clone(),
        name: definition.name.clone(),
        kind: definition.kind.clone(),
        start_byte: definition.start_byte,
    })
}
