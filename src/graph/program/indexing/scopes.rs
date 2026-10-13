use std::cmp::Reverse;
use std::collections::HashSet;

use super::super::{ProgramGraph, ProgramScope, ProgramScopeId};
use crate::model::FileFacts;

impl<'a> ProgramGraph<'a> {
    pub(super) fn index_declared_scopes(
        &mut self,
        file_index: usize,
        fact: &FileFacts,
        scope_ids: &mut HashSet<ProgramScopeId>,
    ) {
        for import in &fact.imports {
            self.add_scope(
                ProgramScopeId {
                    file_index,
                    context_id: import.context_id,
                    start_byte: import.scope_start,
                    end_byte: import.scope_end,
                },
                scope_ids,
            );
        }
        for export in &fact.exports {
            self.add_scope(
                ProgramScopeId {
                    file_index,
                    context_id: export.context_id,
                    start_byte: export.scope_start,
                    end_byte: export.scope_end,
                },
                scope_ids,
            );
        }
        for reference in &fact.references {
            self.add_scope(
                ProgramScopeId {
                    file_index,
                    context_id: reference.context_id,
                    start_byte: reference.scope_start,
                    end_byte: reference.scope_end,
                },
                scope_ids,
            );
        }
    }
    pub(super) fn add_scope(&mut self, id: ProgramScopeId, seen: &mut HashSet<ProgramScopeId>) {
        if seen.insert(id) {
            let index = self.scopes.len();
            self.scopes.push(ProgramScope { id, parent: None });
            self.scopes_by_file[id.file_index].push(index);
            self.scope_by_id.insert(id, index);
        }
    }
    pub(super) fn link_scope_parents(&mut self) {
        for file_scopes in &self.scopes_by_file {
            let mut ordered = file_scopes.clone();
            ordered.sort_by_key(|index| {
                let scope = self.scopes[*index].id;
                (scope.context_id, scope.start_byte, Reverse(scope.end_byte))
            });
            let mut ancestors = Vec::<usize>::new();
            let mut context_id = None;
            for child_index in ordered {
                let child = self.scopes[child_index].id;
                if context_id != Some(child.context_id) {
                    ancestors.clear();
                    context_id = Some(child.context_id);
                }
                while let Some(parent_index) = ancestors.last().copied() {
                    let parent = self.scopes[parent_index].id;
                    if parent.start_byte <= child.start_byte && child.end_byte <= parent.end_byte {
                        break;
                    }
                    ancestors.pop();
                }
                self.scopes[child_index].parent = ancestors
                    .last()
                    .map(|parent_index| self.scopes[*parent_index].id);
                ancestors.push(child_index);
            }
        }
    }
}
