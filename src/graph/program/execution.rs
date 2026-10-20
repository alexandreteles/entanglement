use crate::graph::DeclarationKind::{Class, CompanionObject, Object};
use crate::graph::{CallDispatch, DeclarationExecutionFacts};
use crate::model::{
    ExecutionTarget, Reference, ReferenceAnalysis, ReferenceKind, Resolution, SourceRange,
};

use super::{
    CallEdge, ExecutionGraph, ExecutionRelation, ProgramGraph, ProgramScopeId, ProgramSymbolId,
};

impl<'a> ProgramGraph<'a> {
    pub(crate) fn confirmed_relations(
        &self,
        resolutions: &[Vec<ReferenceAnalysis>],
    ) -> ExecutionGraph {
        let mut calls = Vec::new();
        for (file_index, fact) in self.facts.iter().enumerate() {
            let Some(analyses) = resolutions.get(file_index) else {
                continue;
            };
            for (reference, analysis) in fact.references().zip(analyses) {
                let Some(target) = self.resolved_reference_target(reference, analysis) else {
                    continue;
                };
                self.append_execution_calls(file_index, reference, analysis, target, &mut calls);
                calls.extend(self.legacy_call_edge(file_index, reference, analysis, target));
            }
        }
        calls.extend(self.implicit_initialization_edges());
        calls.sort();
        calls.dedup();
        self.index_execution_relations(calls)
    }

    fn resolved_reference_target(
        &self,
        reference: &Reference,
        analysis: &ReferenceAnalysis,
    ) -> Option<ProgramSymbolId> {
        if analysis.start_byte != reference.start_byte
            || analysis.end_byte != reference.end_byte
            || analysis.path != reference.path
        {
            return None;
        }
        let Resolution::Exact(target) = &analysis.resolution else {
            return None;
        };
        self.unique_symbol_for_target(target)
    }

    fn append_execution_calls(
        &self,
        file_index: usize,
        reference: &Reference,
        analysis: &ReferenceAnalysis,
        target: ProgramSymbolId,
        calls: &mut Vec<CallEdge>,
    ) {
        if analysis.dispatch != Some(CallDispatch::Direct) {
            return;
        }
        calls.extend(
            analysis
                .execution_targets
                .iter()
                .filter(|execution| {
                    self.valid_execution_target(target, execution)
                        && self.is_reported_execution_caller(
                            file_index,
                            reference,
                            execution.caller_start_byte,
                        )
                })
                .map(|execution| CallEdge {
                    relation: ExecutionRelation::Invocation,
                    caller_file: file_index,
                    caller_start_byte: execution.caller_start_byte,
                    callee_file: execution.file_index,
                    callee_start_byte: execution.range.start_byte,
                    call_start_byte: reference.start_byte,
                    call_end_byte: reference.end_byte,
                }),
        );
    }

    fn legacy_call_edge(
        &self,
        file_index: usize,
        reference: &Reference,
        analysis: &ReferenceAnalysis,
        target: ProgramSymbolId,
    ) -> Option<CallEdge> {
        let caller = reference.call_owner?;
        if !is_invocation(reference.kind) {
            return None;
        }
        let symbol = self.symbol(target)?;
        let callee_start = symbol.semantic_id.start_byte;
        let dispatch_is_confirmed = match symbol.kind {
            crate::model::DefinitionKind::Function => {
                matches!(analysis.dispatch, None | Some(CallDispatch::Direct))
            }
            crate::model::DefinitionKind::Method => analysis.dispatch == Some(CallDispatch::Direct),
            _ => false,
        };
        let callable_is_reported = self.function_starts[symbol.id.file_index]
            .contains(&(symbol.scope.context_id, callee_start));
        (dispatch_is_confirmed
            && callable_is_reported
            && self.is_reported_caller(file_index, reference, caller))
        .then_some(CallEdge {
            relation: ExecutionRelation::Invocation,
            caller_file: file_index,
            caller_start_byte: caller,
            callee_file: symbol.id.file_index,
            callee_start_byte: callee_start,
            call_start_byte: reference.start_byte,
            call_end_byte: reference.end_byte,
        })
    }
}

fn is_invocation(kind: ReferenceKind) -> bool {
    matches!(
        kind,
        ReferenceKind::Call
            | ReferenceKind::Method
            | ReferenceKind::Qualified
            | ReferenceKind::Value
    )
}

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
                self.file(file_index).allows(context_id)
                    && (self.callable_scopes[file_index].contains(&ProgramScopeId::new(
                        file_index,
                        context_id,
                        function.start_byte..function.end_byte,
                    )) || self.facts[file_index]
                        .definitions()
                        .filter(|definition| definition.context_id == context_id)
                        .filter_map(|definition| definition.declaration.as_deref())
                        .filter_map(|declaration| declaration.execution.as_ref())
                        .flat_map(execution_ranges)
                        .any(|range| {
                            range.start_byte == function.start_byte
                                && range.end_byte == function.end_byte
                        }))
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
        self.symbol(resolved_symbol)
            .filter(|symbol| {
                target.file_index == symbol.id.file_index
                    && target.context_id == symbol.scope.context_id
            })
            .and_then(|symbol| symbol.definition.declaration.as_deref())
            .and_then(|declaration| declaration.execution.as_ref())
            .is_some_and(|execution| {
                execution_ranges(execution).any(|range| range == target.range)
                    && self.facts[target.file_index]
                        .analysis
                        .functions
                        .iter()
                        .any(|function| {
                            function.start_byte == target.range.start_byte
                                && function.end_byte == target.range.end_byte
                        })
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
impl<'a> ProgramGraph<'a> {
    pub(super) fn implicit_initialization_edges(&self) -> Vec<CallEdge> {
        self.facts
            .iter()
            .enumerate()
            .flat_map(|(file_index, fact)| {
                fact.definitions()
                    .filter_map(move |definition| {
                        Some(self.declaration_initialization_edges(
                            file_index,
                            definition.context_id,
                            definition.declaration.as_deref()?,
                        ))
                    })
                    .flatten()
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
                        relation: ExecutionRelation::Initialization,
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
