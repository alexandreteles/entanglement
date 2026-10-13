use crate::model::Definition;

use super::{ProgramGraph, ProgramScope, ProgramScopeId};

impl<'a> ProgramGraph<'a> {
    pub(crate) fn scopes(&self, file_index: usize) -> impl Iterator<Item = ProgramScope> + '_ {
        self.scopes_by_file[file_index]
            .iter()
            .map(|index| self.scopes[*index])
    }

    pub(crate) fn scope_at(
        &self,
        file_index: usize,
        context_id: usize,
        position: usize,
    ) -> Option<ProgramScopeId> {
        self.scopes(file_index)
            .map(|scope| scope.id)
            .filter(|scope| {
                scope.context_id == context_id
                    && scope.start_byte <= position
                    && position < scope.end_byte
            })
            .min_by_key(|scope| scope.end_byte - scope.start_byte)
    }

    fn scope(&self, id: ProgramScopeId) -> Option<ProgramScope> {
        self.scope_by_id.get(&id).map(|index| self.scopes[*index])
    }

    fn within_callable_scope(
        &self,
        file_index: usize,
        context_id: usize,
        position: usize,
        start_byte: usize,
        end_byte: usize,
    ) -> bool {
        let Some(mut current) = self.scope_at(file_index, context_id, position) else {
            return false;
        };
        loop {
            let Some(scope) = self.scope(current) else {
                return false;
            };
            if current.start_byte < position
                && current.start_byte <= start_byte
                && end_byte <= current.end_byte
                && self.callable_scopes[file_index].contains(&current)
            {
                return true;
            }
            let Some(parent) = scope.parent else {
                return false;
            };
            current = parent;
        }
    }

    pub(crate) fn is_local_definition(&self, file_index: usize, definition: &Definition) -> bool {
        self.within_callable_scope(
            file_index,
            definition.context_id,
            definition.start_byte,
            definition.scope_start,
            definition.scope_end,
        )
    }

    pub(crate) fn is_local_scope(
        &self,
        file_index: usize,
        context_id: usize,
        scope_start: usize,
        scope_end: usize,
        position: usize,
    ) -> bool {
        self.within_callable_scope(file_index, context_id, position, scope_start, scope_end)
    }
}
