use std::collections::HashSet;

use super::super::bindings::binding_identity;
use super::super::{ProgramBinding, ProgramGraph, ProgramScopeId};
use crate::model::FileFacts;

impl<'a> ProgramGraph<'a> {
    pub(super) fn index_local_bindings(
        &mut self,
        file_index: usize,
        fact: &FileFacts,
        scope_ids: &mut HashSet<ProgramScopeId>,
    ) {
        for local in &fact.locals {
            self.add_binding(
                ProgramBinding {
                    file_index,
                    context_id: local.context_id,
                    name: local.name.clone(),
                    start_byte: local.start_byte,
                    end_byte: local.end_byte,
                    scope: ProgramScopeId {
                        file_index,
                        context_id: local.context_id,
                        start_byte: local.scope_start,
                        end_byte: local.scope_end,
                    },
                    type_name: None,
                    alias_target: None,
                    callable_reference: false,
                    immutable: false,
                    semantic_evidence: false,
                    conflicting_evidence: false,
                },
                false,
            );
            self.add_scope(
                ProgramScopeId {
                    file_index,
                    context_id: local.context_id,
                    start_byte: local.scope_start,
                    end_byte: local.scope_end,
                },
                scope_ids,
            );
        }
    }
    pub(super) fn index_semantic_bindings(
        &mut self,
        file_index: usize,
        fact: &FileFacts,
        scope_ids: &mut HashSet<ProgramScopeId>,
    ) {
        for semantic in &fact.semantic {
            for binding in &semantic.bindings {
                self.add_binding(
                    ProgramBinding {
                        file_index,
                        context_id: semantic.context_id,
                        name: binding.name.clone(),
                        start_byte: binding.name_start_byte,
                        end_byte: binding.name_end_byte,
                        scope: ProgramScopeId {
                            file_index,
                            context_id: semantic.context_id,
                            start_byte: binding.scope_start,
                            end_byte: binding.scope_end,
                        },
                        type_name: binding.type_name.clone(),
                        alias_target: binding.alias_target.clone(),
                        callable_reference: binding.callable_reference,
                        immutable: binding.immutable,
                        semantic_evidence: false,
                        conflicting_evidence: false,
                    },
                    true,
                );
                self.add_scope(
                    ProgramScopeId {
                        file_index,
                        context_id: semantic.context_id,
                        start_byte: binding.scope_start,
                        end_byte: binding.scope_end,
                    },
                    scope_ids,
                );
            }
            for receiver in &semantic.receivers {
                self.add_scope(
                    ProgramScopeId {
                        file_index,
                        context_id: semantic.context_id,
                        start_byte: receiver.start_byte,
                        end_byte: receiver.end_byte,
                    },
                    scope_ids,
                );
            }
        }
    }
    fn add_binding(&mut self, mut binding: ProgramBinding, semantic_evidence: bool) {
        let existing = self.bindings_by_file_name[binding.file_index]
            .get(&binding.name)
            .into_iter()
            .flatten()
            .copied()
            .find(|index| binding_identity(&self.bindings[*index]) == binding_identity(&binding));
        if let Some(index) = existing {
            let existing = &mut self.bindings[index];
            if semantic_evidence && !existing.semantic_evidence {
                binding.semantic_evidence = true;
                *existing = binding;
            } else if semantic_evidence && existing.semantic_evidence {
                existing.conflicting_evidence |=
                    conflicting_option(&existing.type_name, &binding.type_name)
                        || conflicting_option(&existing.alias_target, &binding.alias_target)
                        || existing.callable_reference != binding.callable_reference
                        || existing.immutable != binding.immutable;
                existing.type_name = existing.type_name.clone().or(binding.type_name);
                existing.alias_target = existing.alias_target.clone().or(binding.alias_target);
                existing.callable_reference |= binding.callable_reference;
                existing.immutable &= binding.immutable;
            }
            return;
        }
        binding.semantic_evidence = semantic_evidence;
        let index = self.bindings.len();
        self.bindings_by_file_name[binding.file_index]
            .entry(binding.name.clone())
            .or_default()
            .push(index);
        self.bindings.push(binding);
    }
}

fn conflicting_option<T: PartialEq>(left: &Option<T>, right: &Option<T>) -> bool {
    matches!((left, right), (Some(left), Some(right)) if left != right)
}
