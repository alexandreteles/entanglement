use crate::graph::CallDispatch;
use crate::model::{Reference, ReferenceAnalysis, ReferenceKind, Resolution};

use super::{CallEdge, ProgramGraph, ProgramRelation, ProgramRelationKind, ProgramSymbolId};

impl<'a> ProgramGraph<'a> {
    pub(crate) fn confirmed_relations(
        &mut self,
        resolutions: &[Vec<ReferenceAnalysis>],
    ) -> Vec<CallEdge> {
        let mut calls = Vec::new();
        let mut relations = Vec::new();
        for (file_index, fact) in self.facts.iter().enumerate() {
            let Some(analyses) = resolutions.get(file_index) else {
                continue;
            };
            for (reference, analysis) in fact.references.iter().zip(analyses) {
                self.append_reference(file_index, reference, analysis, &mut relations, &mut calls);
            }
        }
        self.relations = relations;
        calls.extend(self.relation_call_edges());
        calls.extend(self.implicit_initialization_edges());
        calls.sort_by_key(call_order);
        calls.dedup();
        calls
    }

    fn append_reference(
        &self,
        file_index: usize,
        reference: &Reference,
        analysis: &ReferenceAnalysis,
        relations: &mut Vec<ProgramRelation>,
        calls: &mut Vec<CallEdge>,
    ) {
        let Some(target) = self.resolved_reference_target(reference, analysis) else {
            return;
        };
        self.append_execution_calls(file_index, reference, analysis, target, calls);
        let caller = self.legacy_caller(file_index, reference, analysis, target);
        relations.push(ProgramRelation {
            source_file: file_index,
            source_start_byte: reference.start_byte,
            source_end_byte: reference.end_byte,
            caller_start_byte: caller,
            target,
            kind: if caller.is_some() {
                ProgramRelationKind::Call
            } else {
                ProgramRelationKind::Reference
            },
        });
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
        for execution in &analysis.execution_targets {
            if !self.valid_execution_target(target, execution)
                || !self.is_reported_execution_caller(
                    file_index,
                    reference,
                    execution.caller_start_byte,
                )
            {
                continue;
            }
            calls.push(CallEdge {
                caller_file: file_index,
                caller_start_byte: execution.caller_start_byte,
                callee_file: execution.file_index,
                callee_start_byte: execution.range.start_byte,
                call_start_byte: reference.start_byte,
                call_end_byte: reference.end_byte,
            });
        }
    }

    fn legacy_caller(
        &self,
        file_index: usize,
        reference: &Reference,
        analysis: &ReferenceAnalysis,
        target: ProgramSymbolId,
    ) -> Option<usize> {
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
        .then_some(caller)
    }

    fn relation_call_edges(&self) -> impl Iterator<Item = CallEdge> + '_ {
        self.relations.iter().filter_map(|relation| {
            if relation.kind != ProgramRelationKind::Call {
                return None;
            }
            let caller_start_byte = relation.caller_start_byte?;
            let callee = self.symbol(relation.target)?;
            Some(CallEdge {
                caller_file: relation.source_file,
                caller_start_byte,
                callee_file: callee.id.file_index,
                callee_start_byte: callee.semantic_id.start_byte,
                call_start_byte: relation.source_start_byte,
                call_end_byte: relation.source_end_byte,
            })
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

fn call_order(edge: &CallEdge) -> (usize, usize, usize, usize, usize, usize) {
    (
        edge.caller_file,
        edge.caller_start_byte,
        edge.callee_file,
        edge.callee_start_byte,
        edge.call_start_byte,
        edge.call_end_byte,
    )
}
