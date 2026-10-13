use crate::graph::DeclarationKind::{Class, CompanionObject, Object};

use super::{CallEdge, ProgramGraph};

impl<'a> ProgramGraph<'a> {
    pub(super) fn implicit_initialization_edges(&self) -> Vec<CallEdge> {
        self.facts
            .iter()
            .enumerate()
            .flat_map(|(file_index, fact)| {
                fact.semantic.iter().flat_map(move |semantic| {
                    semantic.declarations.iter().flat_map(move |declaration| {
                        self.declaration_initialization_edges(
                            file_index,
                            semantic.context_id,
                            declaration,
                        )
                    })
                })
            })
            .collect()
    }

    fn declaration_initialization_edges(
        &self,
        file_index: usize,
        context_id: usize,
        declaration: &crate::graph::DeclarationFacts,
    ) -> Vec<CallEdge> {
        if !matches!(declaration.kind, Class | Object | CompanionObject) {
            return Vec::new();
        }
        let Some(execution) = declaration.execution.as_ref() else {
            return Vec::new();
        };
        let constructors =
            self.reported_execution_ranges(file_index, context_id, &execution.constructors);
        let initializers =
            self.reported_execution_ranges(file_index, context_id, &execution.initializers);
        constructors
            .into_iter()
            .flat_map(|constructor| {
                initializers
                    .iter()
                    .copied()
                    .map(move |initializer| CallEdge {
                        caller_file: file_index,
                        caller_start_byte: constructor.start_byte,
                        callee_file: file_index,
                        callee_start_byte: initializer.start_byte,
                        call_start_byte: constructor.start_byte,
                        call_end_byte: constructor.end_byte,
                    })
            })
            .collect()
    }

    fn reported_execution_ranges(
        &self,
        file_index: usize,
        context_id: usize,
        ranges: &[crate::model::SourceRange],
    ) -> Vec<crate::model::SourceRange> {
        ranges
            .iter()
            .copied()
            .filter(|range| self.is_reported_execution_range(file_index, context_id, *range))
            .collect()
    }

    fn is_reported_execution_range(
        &self,
        file_index: usize,
        context_id: usize,
        range: crate::model::SourceRange,
    ) -> bool {
        self.is_reported_callable(file_index, context_id, range.start_byte)
            && self.facts[file_index]
                .analysis
                .functions
                .iter()
                .any(|function| {
                    function.start_byte == range.start_byte && function.end_byte == range.end_byte
                })
    }
}
