use std::collections::HashSet;

use super::{ProgramBinding, ProgramGraph, binding_identity};

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
            BindingAlias::Path {
                callable_reference: false,
                ..
            }
            | BindingAlias::Type(_) => None,
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
                return Some(BindingAlias::Type(type_name.clone()));
            }
            if !current.immutable || !seen.insert(binding_identity(current)) {
                return None;
            }
            callable_reference |= current.callable_reference;
            let target = current.alias_target.as_deref()?;
            if !is_simple_binding_name(target) {
                let first = target.split('.').next()?;
                return self.terminal_alias_path(
                    target,
                    first,
                    current.end_byte,
                    callable_reference,
                    file_index,
                    context_id,
                );
            }

            let Some(next) = self
                .binding_at(file_index, target, context_id, current.end_byte)
                .filter(|binding| resolve_type || has_alias(binding))
            else {
                return self.terminal_alias_path(
                    target,
                    target,
                    current.end_byte,
                    callable_reference,
                    file_index,
                    context_id,
                );
            };
            current = next;
        }
    }

    fn terminal_alias_path(
        &self,
        path: &str,
        first: &str,
        position: usize,
        callable_reference: bool,
        file_index: usize,
        context_id: usize,
    ) -> Option<BindingAlias> {
        (!self.has_local_binding(file_index, first, context_id, position)).then(|| {
            BindingAlias::Path {
                path: path.to_owned(),
                callable_reference,
            }
        })
    }
}

enum BindingAlias {
    Type(String),
    Path {
        path: String,
        callable_reference: bool,
    },
}

fn is_simple_binding_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('.')
}

fn has_alias(binding: &ProgramBinding) -> bool {
    binding.immutable && binding.alias_target.is_some()
}
