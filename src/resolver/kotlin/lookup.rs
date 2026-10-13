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
        let local = self.local_functions(file, reference, name);
        if !local.is_empty() {
            return self.accessible(file, reference, local);
        }
        if reference.kind == ReferenceKind::Call {
            let local = self.local_callable_values(file, reference, name);
            if !local.is_empty() {
                return self.accessible(file, reference, local);
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
                    declaration.kind == DeclarationKind::Property
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
                    declaration.kind == DeclarationKind::Function
                        && !declaration.modifiers.is_open
                        && !declaration.modifiers.is_abstract
                        && !declaration.modifiers.is_expect
                })
            })
            .map(|candidate| candidate.candidate_index)
            .filter(|index| !self.accessible(file, reference, vec![*index]).is_empty())
            .collect()
    }
    pub(super) fn namespace_receiver_candidates(
        &self,
        file: usize,
        reference: &Reference,
        receiver: &str,
        name: &str,
    ) -> Option<Vec<usize>> {
        self.imports(file, reference.context_id)
            .find(|import| imported_name(import) == Some(receiver) && import.namespace)
            .map(|import| {
                let candidates = self
                    .named(name)
                    .filter(|candidate| {
                        candidate.package == import.path && candidate.owner.is_empty()
                    })
                    .map(|candidate| candidate.candidate_index)
                    .collect();
                self.accessible(file, reference, candidates)
            })
    }

    pub(super) fn imported_namespace_path_candidates(
        &self,
        file: usize,
        reference: &Reference,
        path: &[String],
    ) -> Option<Vec<usize>> {
        let name = path.last()?;
        self.imports(file, reference.context_id)
            .filter(|import| {
                import.namespace && imported_name(import) == path.first().map(String::as_str)
            })
            .find_map(|import| {
                let qualified = import
                    .path
                    .iter()
                    .chain(path.iter().skip(1))
                    .cloned()
                    .collect::<Vec<_>>();
                (1..qualified.len()).rev().find_map(|split| {
                    let candidates = self
                        .named(name)
                        .filter(|candidate| {
                            candidate.package == qualified[..split] && candidate.owner.is_empty()
                        })
                        .map(|candidate| candidate.candidate_index)
                        .collect::<Vec<_>>();
                    (!candidates.is_empty()).then(|| self.accessible(file, reference, candidates))
                })
            })
    }

    pub(super) fn imported_candidates(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
        wildcard_only: bool,
    ) -> Vec<usize> {
        self.imports(file, reference.context_id)
            .filter_map(|import| match (import.namespace, wildcard_only) {
                (true, true) => Some((import, name)),
                (false, false) if imported_name(import) == Some(name) => Some((
                    import,
                    import.path.last().map(String::as_str).unwrap_or(name),
                )),
                _ => None,
            })
            .flat_map(|(import, imported)| {
                self.named(imported)
                    .filter(move |candidate| match import.namespace {
                        true => candidate.package == import.path && candidate.owner.is_empty(),
                        false => candidate_type_segments(candidate) == import.path,
                    })
                    .filter(|candidate| !self.is_local_candidate(candidate))
                    .filter(|candidate| self.visible_in_source_set(file, candidate))
                    .map(|candidate| candidate.candidate_index)
            })
            .collect()
    }

    pub(super) fn qualified_candidates(
        &self,
        file: usize,
        reference: &Reference,
        path: &[String],
        package: &[String],
    ) -> Vec<usize> {
        match path {
            [] | [_] => Vec::new(),
            [receiver, name] => self
                .two_segment_receiver_candidates(file, reference, path, package)
                .unwrap_or_else(|| {
                    self.single_segment_package_candidates(file, reference, receiver, name)
                }),
            [receiver, .., name] => {
                if self.graph.has_local_binding(
                    file,
                    receiver,
                    reference.context_id,
                    reference.start_byte,
                ) {
                    Vec::new()
                } else {
                    self.long_qualified_candidates(file, reference, path, name)
                }
            }
        }
    }

    pub(super) fn two_segment_receiver_candidates(
        &self,
        file: usize,
        reference: &Reference,
        path: &[String],
        package: &[String],
    ) -> Option<Vec<usize>> {
        let receiver = &path[0];
        let name = path.last()?;
        if is_explicit_receiver(receiver) {
            return Some(
                self.explicit_receiver_candidates(file, reference, receiver, name, package),
            );
        }

        if self
            .graph
            .has_local_binding(file, receiver, reference.context_id, reference.start_byte)
        {
            let ty = self
                .graph
                .binding_type(file, receiver, reference.context_id, reference.start_byte)
                .or_else(|| self.inferred_lambda_parameter_type(file, reference, receiver));
            return Some(ty.map_or_else(Vec::new, |ty| {
                self.typed_receiver_candidates(file, reference, name, &ty, true)
            }));
        }
        if let Some(ty) = self.inferred_lambda_parameter_type(file, reference, receiver) {
            return Some(self.typed_receiver_candidates(file, reference, name, &ty, true));
        }
        if let Some(ty) = self.imported_type(file, reference, receiver) {
            return Some(self.typed_receiver_candidates(file, reference, name, &ty, false));
        }
        if let Some(ty) = self.same_package_type(package, receiver) {
            let allow_extensions = !self.is_static_type_qualifier(file, reference);
            return Some(self.typed_receiver_candidates(
                file,
                reference,
                name,
                &ty,
                allow_extensions,
            ));
        }

        self.namespace_receiver_candidates(file, reference, receiver, name)
    }
    pub(super) fn explicit_receiver_candidates(
        &self,
        file: usize,
        reference: &Reference,
        receiver: &str,
        name: &str,
        package: &[String],
    ) -> Vec<usize> {
        let scopes = self.active_receivers(file, reference);
        let scope = receiver
            .strip_prefix("this@")
            .and_then(|label| {
                scopes
                    .iter()
                    .find(|scope| scope.label.as_deref() == Some(label))
            })
            .or_else(|| {
                (!receiver.starts_with("this@"))
                    .then(|| scopes.first())
                    .flatten()
            });
        if let Some(scope) = scope {
            return self.typed_receiver_candidates(
                file,
                reference,
                name,
                &scope.receiver_type,
                true,
            );
        }
        let Some(label) = receiver.strip_prefix("this@") else {
            return Vec::new();
        };
        let Some(class) = self
            .named(label)
            .find(|candidate| candidate.package == package && is_type_declaration(candidate))
        else {
            return Vec::new();
        };
        self.accessible(
            file,
            reference,
            self.members_of_type(file, reference, name, &class.name),
        )
    }
    pub(super) fn typed_receiver_candidates(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
        type_name: &str,
        allow_extensions: bool,
    ) -> Vec<usize> {
        let mut candidates = self.members_of_type(file, reference, name, type_name);
        if candidates.is_empty() && allow_extensions {
            candidates = self.extensions_for_type(file, reference, name, type_name);
        }
        self.accessible(file, reference, candidates)
    }
    pub(super) fn same_package_type(&self, package: &[String], receiver: &str) -> Option<String> {
        let candidates = self
            .named(receiver)
            .filter(|candidate| {
                candidate.package == package
                    && candidate.owner.is_empty()
                    && is_type_declaration(candidate)
            })
            .map(candidate_type_name)
            .collect::<BTreeSet<_>>();
        (candidates.len() == 1)
            .then(|| candidates.into_iter().next())
            .flatten()
    }
    pub(super) fn single_segment_package_candidates(
        &self,
        file: usize,
        reference: &Reference,
        receiver: &str,
        name: &str,
    ) -> Vec<usize> {
        let candidates = self
            .named(name)
            .filter(|candidate| candidate.package == [receiver.to_owned()])
            .filter(|candidate| candidate.owner.is_empty())
            .filter(|candidate| self.visible_in_source_set(file, candidate))
            .map(|candidate| candidate.candidate_index)
            .collect();
        self.accessible(file, reference, candidates)
    }
    pub(super) fn long_qualified_candidates(
        &self,
        file: usize,
        reference: &Reference,
        path: &[String],
        name: &str,
    ) -> Vec<usize> {
        if let Some(candidates) =
            self.fully_qualified_package_candidates(file, reference, path, name)
        {
            return candidates;
        }
        if let Some(ty) = self.imported_type(file, reference, &path[0]) {
            return self.typed_receiver_candidates(file, reference, name, &ty, false);
        }
        // Unknown qualified receivers never match a top-level declaration by
        // their final segment.
        self.imported_namespace_path_candidates(file, reference, path)
            .unwrap_or_default()
    }
    pub(super) fn fully_qualified_package_candidates(
        &self,
        file: usize,
        reference: &Reference,
        path: &[String],
        name: &str,
    ) -> Option<Vec<usize>> {
        let prefix = &path[..path.len().saturating_sub(1)];
        let candidates = self
            .named(name)
            .filter(|candidate| candidate.package == prefix && candidate.owner.is_empty())
            .filter(|candidate| self.visible_in_source_set(file, candidate))
            .map(|candidate| candidate.candidate_index)
            .collect::<Vec<_>>();
        (!candidates.is_empty()).then(|| self.accessible(file, reference, candidates))
    }
}
