use crate::graph::ProgramGraph;
use crate::model::{DefinitionKind, Export};

use super::bindings::best_ranked;
use super::{Target, unique_targets};

pub(super) fn local_symbols(
    graph: &ProgramGraph<'_>,
    file_index: usize,
    name: &str,
    position: usize,
    context_id: usize,
) -> Option<(usize, Vec<Target>)> {
    let (rank, candidates) = best_ranked(
        graph
            .symbols_named(file_index, name)
            .filter(|symbol| {
                symbol.kind != DefinitionKind::Method
                    && symbol.scope.start_byte <= position
                    && position < symbol.scope.end_byte
                    && symbol.scope.context_id == context_id
            })
            .map(|symbol| {
                (
                    symbol
                        .scope
                        .end_byte
                        .saturating_sub(symbol.scope.start_byte),
                    symbol,
                )
            }),
    )?;
    Some((
        rank,
        unique_targets(
            candidates
                .into_iter()
                .map(|symbol| Target::Symbol(symbol.semantic_id.clone()))
                .collect(),
        ),
    ))
}

pub(super) fn anonymous_export_symbols(
    graph: &ProgramGraph<'_>,
    file_index: usize,
    export: &Export,
) -> Vec<Target> {
    let definitions = graph
        .symbols_for_file(file_index)
        .filter(|symbol| {
            symbol.kind != DefinitionKind::Method
                && symbol.scope.context_id == export.context_id
                && export.start_byte <= symbol.semantic_id.start_byte
                && graph
                    .definition(symbol.id)
                    .is_some_and(|definition| definition.end_byte <= export.end_byte)
        })
        .map(|symbol| Target::Symbol(symbol.semantic_id.clone()))
        .collect();
    unique_targets(definitions)
}
