use crate::graph::DeclarationExecutionFacts;
use crate::model::{DefinitionKind, ExecutionTarget, Reference, SourceRange};

use super::{ProgramGraph, ProgramSymbolId};

impl<'a> ProgramGraph<'a> {
    pub(super) fn is_reported_callable(
        &self,
        file_index: usize,
        context_id: usize,
        start_byte: usize,
    ) -> bool {
        self.facts[file_index]
            .analysis
            .functions
            .iter()
            .filter(|function| function.start_byte == start_byte)
            .any(|function| {
                let declared_function = self.function_starts[file_index]
                    .contains(&(context_id, start_byte))
                    && self
                        .symbols_for_file(file_index)
                        .filter(|symbol| {
                            symbol.scope.context_id == context_id
                                && symbol.semantic_id.start_byte == start_byte
                                && matches!(
                                    symbol.kind,
                                    DefinitionKind::Function | DefinitionKind::Method
                                )
                        })
                        .filter_map(|symbol| self.definition(symbol.id))
                        .any(|definition| definition.end_byte == function.end_byte);
                let declared_execution = self.facts[file_index]
                    .semantic
                    .iter()
                    .filter(|semantic| semantic.context_id == context_id)
                    .flat_map(|semantic| semantic.declarations.iter())
                    .filter_map(|declaration| declaration.execution.as_ref())
                    .flat_map(execution_ranges)
                    .any(|range| {
                        range.start_byte == function.start_byte
                            && range.end_byte == function.end_byte
                    });
                declared_function || declared_execution
            })
    }

    pub(super) fn is_reported_caller(
        &self,
        file_index: usize,
        reference: &Reference,
        caller_start_byte: usize,
    ) -> bool {
        reference.call_owner == Some(caller_start_byte)
            && self.is_reported_scope_owner(file_index, reference, caller_start_byte)
    }

    pub(super) fn is_reported_execution_caller(
        &self,
        file_index: usize,
        reference: &Reference,
        caller_start_byte: usize,
    ) -> bool {
        reference.execution_owner.or(reference.call_owner) == Some(caller_start_byte)
            && self.is_reported_scope_owner(file_index, reference, caller_start_byte)
    }

    fn is_reported_scope_owner(
        &self,
        file_index: usize,
        reference: &Reference,
        caller_start_byte: usize,
    ) -> bool {
        if reference.scope_start > reference.start_byte || reference.end_byte > reference.scope_end
        {
            return false;
        }
        let nearest = self.facts[file_index]
            .analysis
            .functions
            .iter()
            .filter(|function| {
                function.start_byte <= reference.start_byte
                    && reference.end_byte <= function.end_byte
                    && self.is_reported_callable(
                        file_index,
                        reference.context_id,
                        function.start_byte,
                    )
            })
            .min_by_key(|function| function.end_byte.saturating_sub(function.start_byte));
        nearest.is_some_and(|function| function.start_byte == caller_start_byte)
    }

    pub(super) fn valid_execution_target(
        &self,
        resolved_symbol: ProgramSymbolId,
        target: &ExecutionTarget,
    ) -> bool {
        let Some(symbol) = self.symbol(resolved_symbol) else {
            return false;
        };
        if target.file_index != symbol.id.file_index || target.context_id != symbol.scope.context_id
        {
            return false;
        }
        let Some(definition) = self.definition(resolved_symbol) else {
            return false;
        };
        let Some(declaration) = self.declaration(
            symbol.id.file_index,
            symbol.scope.context_id,
            definition.start_byte,
            None,
        ) else {
            return false;
        };
        let Some(execution) = declaration.execution.as_ref() else {
            return false;
        };
        let range_is_declared = execution_ranges(execution).any(|range| range == target.range);
        range_is_declared
            && self.facts[target.file_index]
                .analysis
                .functions
                .iter()
                .any(|function| {
                    function.start_byte == target.range.start_byte
                        && function.end_byte == target.range.end_byte
                })
    }
}

fn execution_ranges(
    execution: &DeclarationExecutionFacts,
) -> impl Iterator<Item = SourceRange> + '_ {
    [execution.callable_value, execution.getter, execution.setter]
        .into_iter()
        .flatten()
        .chain(execution.constructors.iter().copied())
        .chain(execution.initializers.iter().copied())
}
