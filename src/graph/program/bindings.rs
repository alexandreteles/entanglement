//! Select lexical scopes and follow immutable local bindings.

use std::collections::HashSet;

use super::{ProgramBinding, ProgramGraph, ProgramScope, ProgramScopeId};
use crate::model::Definition;

impl<'a> ProgramGraph<'a> {
    pub(crate) fn scopes(&self, file_index: usize) -> impl Iterator<Item = ProgramScope> + '_ {
        self.scopes_by_file[file_index]
            .iter()
            .map(|index| self.scopes[*index])
            .filter(move |scope| self.file(file_index).allows(scope.id.context_id))
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
        self.scope_by_id
            .get(&id)
            .filter(|_| self.file(id.file_index).allows(id.context_id))
            .map(|index| self.scopes[*index])
    }

    fn within_callable_scope(
        &self,
        file_index: usize,
        context_id: usize,
        position: usize,
        start_byte: usize,
        end_byte: usize,
    ) -> bool {
        let nearest = self
            .scope_at(file_index, context_id, position)
            .and_then(|id| self.scope(id));
        std::iter::successors(nearest, |scope| self.scope(scope.parent?)).any(|scope| {
            let current = scope.id;
            current.start_byte < position
                && current.start_byte <= start_byte
                && end_byte <= current.end_byte
                && self.callable_scopes[file_index].contains(&current)
        })
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
impl<'a> ProgramGraph<'a> {
    pub(crate) fn bindings_named(
        &self,
        file_index: usize,
        name: &str,
        context_id: usize,
        position: usize,
    ) -> impl Iterator<Item = &ProgramBinding<'a>> {
        self.bindings_by_file_name[file_index]
            .get(name)
            .into_iter()
            .flatten()
            .map(|index| &self.bindings[*index])
            .filter(move |binding| {
                self.file(file_index).allows(binding.scope.context_id)
                    && binding.scope.context_id == context_id
                    && binding.local.start_byte <= position
                    && binding.scope.start_byte <= position
                    && position < binding.scope.end_byte
            })
    }

    /// Return the unique nearest binding for a name at a source position.
    /// A narrower lexical scope wins; declarations in that same scope use
    /// the latest declaration before the position. Equal-ranked alternatives
    /// are ambiguous and do not provide evidence.
    pub(super) fn binding_at(
        &self,
        file_index: usize,
        name: &str,
        context_id: usize,
        position: usize,
    ) -> Option<&ProgramBinding<'a>> {
        let mut nearest = self
            .bindings_named(file_index, name, context_id, position)
            .collect::<Vec<_>>();
        let narrowest = nearest.iter().map(|binding| binding.scope.width()).min()?;
        nearest.retain(|binding| binding.scope.width() == narrowest);
        let scope = nearest.first()?.scope;
        if nearest.iter().any(|binding| {
            binding.scope.start_byte != scope.start_byte || binding.scope.end_byte != scope.end_byte
        }) {
            return None;
        }
        let latest = nearest
            .iter()
            .map(|binding| binding.local.start_byte)
            .max()?;
        let mut winners = nearest
            .into_iter()
            .filter(|binding| binding.local.start_byte == latest);
        let winner = winners.next()?;
        (winners.next().is_none() && !winner.conflicting_evidence).then_some(winner)
    }

    pub(crate) fn has_local_binding(
        &self,
        file_index: usize,
        name: &str,
        context_id: usize,
        position: usize,
    ) -> bool {
        self.bindings_named(file_index, name, context_id, position)
            .next()
            .is_some()
    }
}

pub(super) fn binding_identity<'a>(
    binding: &ProgramBinding<'a>,
) -> (ProgramScopeId, &'a str, usize, usize) {
    (
        binding.scope,
        binding.local.name.as_str(),
        binding.local.start_byte,
        binding.local.end_byte,
    )
}
impl<'a> ProgramGraph<'a> {
    /// Resolve a binding's declared type, following only bounded immutable
    /// aliases that are visible at each alias declaration in the same
    /// semantic context.
    pub(crate) fn binding_type(
        &self,
        file_index: usize,
        name: &str,
        context_id: usize,
        position: usize,
    ) -> Option<String> {
        match self.follow_alias(file_index, name, context_id, position, true)? {
            BindingAlias::Type(type_name) => Some(type_name),
            BindingAlias::Path { .. } => None,
        }
    }

    /// Follow a simple immutable alias chain and return its terminal source
    /// path. The terminal name is left for the owning resolver to check
    /// against its imports, package, and visibility rules.
    pub(crate) fn binding_alias(
        &self,
        file_index: usize,
        name: &str,
        context_id: usize,
        position: usize,
    ) -> Option<String> {
        match self.follow_alias(file_index, name, context_id, position, false)? {
            BindingAlias::Path { path, .. } => Some(path),
            BindingAlias::Type(_) => None,
        }
    }

    /// Return the terminal path only when an immutable alias chain begins at
    /// an explicit callable-reference value such as `::handler`.
    pub(crate) fn binding_callable_alias(
        &self,
        file_index: usize,
        name: &str,
        context_id: usize,
        position: usize,
    ) -> Option<String> {
        match self.follow_alias(file_index, name, context_id, position, false)? {
            BindingAlias::Path {
                path,
                callable_reference: true,
            } => Some(path),
            _ => None,
        }
    }

    fn follow_alias(
        &self,
        file_index: usize,
        name: &str,
        context_id: usize,
        position: usize,
        resolve_type: bool,
    ) -> Option<BindingAlias> {
        let mut current = self.binding_at(file_index, name, context_id, position)?;
        let mut seen = HashSet::new();
        let mut callable_reference = false;
        loop {
            if let Some(type_name) = current.type_name.as_ref().filter(|_| resolve_type) {
                return Some(BindingAlias::Type((*type_name).to_owned()));
            }
            if !current.immutable || !seen.insert(binding_identity(current)) {
                return None;
            }
            callable_reference |= current.callable_reference;
            let target = current.alias_target.as_deref()?;
            let next = (!target.is_empty() && !target.contains('.'))
                .then(|| self.binding_at(file_index, target, context_id, current.local.end_byte))
                .flatten()
                .filter(|binding| {
                    resolve_type || (binding.immutable && binding.alias_target.is_some())
                });
            let Some(next) = next else {
                return (!self.has_local_binding(
                    file_index,
                    target.split('.').next()?,
                    context_id,
                    current.local.end_byte,
                ))
                .then(|| BindingAlias::Path {
                    path: target.to_owned(),
                    callable_reference,
                });
            };
            current = next;
        }
    }
}

enum BindingAlias {
    Type(String),
    Path {
        path: String,
        callable_reference: bool,
    },
}

#[cfg(test)]
#[path = "../../../tests/unit/program_projection.rs"]
mod projection_tests;
