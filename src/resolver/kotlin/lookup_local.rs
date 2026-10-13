use super::*;

impl Index<'_> {
    pub(super) fn simple_candidates(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
        package: &[String],
    ) -> Vec<usize> {
        let position = reference.start_byte;
        if let Some(candidates) = self.local_function_lookup(file, reference, name) {
            return candidates;
        }
        if reference.kind == ReferenceKind::Call {
            if let Some(candidates) = self.local_callable_value_lookup(file, reference, name) {
                return candidates;
            }
            if let Some(candidates) = self.callable_alias_lookup(file, reference, name, package) {
                return self.accessible(file, reference, candidates);
            }
        }
        if let Some(alias) = self
            .graph
            .binding_alias(file, name, reference.context_id, position)
        {
            let candidates = self.alias_target_candidates(file, reference, &alias, package);
            return self.accessible(file, reference, candidates);
        }
        if self
            .graph
            .has_local_binding(file, name, reference.context_id, position)
        {
            return Vec::new();
        }
        self.unqualified_candidates(file, reference, name, package)
    }
    fn local_function_lookup(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
    ) -> Option<Vec<usize>> {
        let candidates = self.local_functions(file, reference, name);
        (!candidates.is_empty()).then(|| self.accessible(file, reference, candidates))
    }
    fn local_callable_value_lookup(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
    ) -> Option<Vec<usize>> {
        let candidates = self.local_callable_values(file, reference, name);
        (!candidates.is_empty()).then(|| self.accessible(file, reference, candidates))
    }
    fn unqualified_candidates(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
        package: &[String],
    ) -> Vec<usize> {
        if let Some(receiver) = self.implicit_receiver_candidates(file, reference, name)
            && !receiver.is_empty()
        {
            return receiver;
        }
        let imported = self.non_extension_imports(file, reference, name);
        if !imported.is_empty() {
            return self.accessible(file, reference, imported);
        }
        // Calls inside a lambda passed to a callable whose receiver type is
        // unknown cannot borrow same-package declarations as implicit DSL
        // members. A statically proven receiver has already been considered
        // above and ordinary explicit imports remain usable.
        if self.inside_unresolved_lambda_argument(file, reference) {
            return Vec::new();
        }
        let in_package = self.package_candidates(file, name, package);
        if !in_package.is_empty() {
            return self.accessible(file, reference, in_package);
        }
        self.accessible(
            file,
            reference,
            self.wildcard_candidates(file, reference, name),
        )
    }
    pub(super) fn local_functions(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
    ) -> Vec<usize> {
        self.in_scope_local_candidates(file, reference, name)
            .filter(|candidate| {
                matches!(
                    candidate.kind,
                    DefinitionKind::Function | DefinitionKind::Method
                ) && self.is_local_candidate(candidate)
            })
            .map(|candidate| candidate.candidate_index)
            .collect()
    }
    pub(super) fn in_scope_local_candidates<'b>(
        &'b self,
        file: usize,
        reference: &'b Reference,
        name: &'b str,
    ) -> impl Iterator<Item = &'b Candidate> + 'b {
        let position = reference.start_byte;
        self.named(name).filter(move |candidate| {
            candidate.file == file
                && candidate.context_id == reference.context_id
                && candidate.start < position
                && candidate.scope_start <= position
                && position < candidate.scope_end
        })
    }
    pub(super) fn local_callable_values(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
    ) -> Vec<usize> {
        self.in_scope_local_candidates(file, reference, name)
            .filter(|candidate| {
                candidate.declaration.as_ref().is_some_and(|declaration| {
                    declaration.kind == crate::graph::DeclarationKind::Property
                        && declaration
                            .execution
                            .as_ref()
                            .is_some_and(|execution| execution.callable_value.is_some())
                }) && self.is_local_candidate(candidate)
            })
            .map(|candidate| candidate.candidate_index)
            .collect()
    }
    pub(super) fn callable_alias_lookup(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
        package: &[String],
    ) -> Option<Vec<usize>> {
        if reference.kind != ReferenceKind::Call {
            return None;
        }
        let target = self.graph.binding_callable_alias(
            file,
            name,
            reference.context_id,
            reference.start_byte,
        )?;
        Some(self.callable_alias_candidates(file, reference, &target, package))
    }
    pub(super) fn alias_target_candidates(
        &self,
        file: usize,
        reference: &Reference,
        target: &str,
        package: &[String],
    ) -> Vec<usize> {
        let mut segments = target.split('.').map(str::to_owned).collect::<Vec<_>>();
        let target_name = segments.pop().unwrap_or_default();
        let imported = self.imported_candidates(file, reference, &target_name, false);
        if !imported.is_empty() {
            return imported;
        }
        let target_package = if segments.is_empty() {
            package.to_vec()
        } else {
            segments
        };
        self.named(&target_name)
            .filter(|candidate| candidate.package == target_package && candidate.owner.is_empty())
            .filter(|candidate| self.visible_in_source_set(file, candidate))
            .map(|candidate| candidate.candidate_index)
            .collect()
    }
    pub(super) fn non_extension_imports(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
    ) -> Vec<usize> {
        self.imported_candidates(file, reference, name, false)
            .into_iter()
            .filter(|index| !is_extension_candidate(&self.candidates[*index]))
            .collect()
    }
    pub(super) fn package_candidates(
        &self,
        file: usize,
        name: &str,
        package: &[String],
    ) -> Vec<usize> {
        self.named(name)
            .filter(|candidate| candidate.package == package && candidate.owner.is_empty())
            .filter(|candidate| !self.is_local_candidate(candidate))
            .filter(|candidate| !is_extension_candidate(candidate))
            .filter(|candidate| self.visible_in_source_set(file, candidate))
            .map(|candidate| candidate.candidate_index)
            .collect()
    }
    pub(super) fn wildcard_candidates(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
    ) -> Vec<usize> {
        self.imported_candidates(file, reference, name, true)
            .into_iter()
            .filter(|index| {
                !is_extension_candidate(&self.candidates[*index])
                    && !self.is_local_candidate(&self.candidates[*index])
            })
            .collect()
    }
    pub(super) fn callable_alias_candidates(
        &self,
        file: usize,
        reference: &Reference,
        target: &str,
        package: &[String],
    ) -> Vec<usize> {
        let segments = target.split('.').map(str::to_owned).collect::<Vec<_>>();
        let Some(name) = segments.last() else {
            return Vec::new();
        };
        let target_package = if segments.len() == 1 {
            package.to_vec()
        } else {
            segments[..segments.len() - 1].to_vec()
        };
        self.named(name)
            .filter(|candidate| candidate.package == target_package && candidate.owner.is_empty())
            .filter(|candidate| {
                candidate.declaration.as_ref().is_some_and(|declaration| {
                    declaration.kind == crate::graph::DeclarationKind::Function
                        && !declaration.modifiers.is_open
                        && !declaration.modifiers.is_abstract
                        && !declaration.modifiers.is_expect
                })
            })
            .map(|candidate| candidate.candidate_index)
            .filter(|index| !self.accessible(file, reference, vec![*index]).is_empty())
            .collect()
    }
}
