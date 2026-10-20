use super::*;

mod lambdas;

impl Index<'_> {
    pub(super) fn lexical_receivers(
        &self,
        file: usize,
        reference: &Reference,
    ) -> Option<Vec<ReceiverScope>> {
        let semantic = self.graph.semantic(
            file,
            reference.context_id,
            Some(ResolutionFamily::KotlinModules),
        );
        semantic.clone().next()?;
        Some(
            semantic
                .flat_map(|semantic| &semantic.semantic.receivers)
                .filter(|scope| {
                    scope.implicit
                        && scope.start_byte <= reference.start_byte
                        && reference.start_byte < scope.end_byte
                })
                .cloned()
                .collect(),
        )
    }

    pub(super) fn implicit_receiver_candidates(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
    ) -> Option<Vec<usize>> {
        self.implicit_receiver_candidates_from_scopes(
            file,
            reference,
            name,
            self.active_receivers(file, reference),
        )
    }
    pub(super) fn implicit_receiver_candidates_from_scopes(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
        scopes: Vec<ReceiverScope>,
    ) -> Option<Vec<usize>> {
        let mut seen_markers = HashSet::new();
        for scope in scopes {
            if scope
                .dsl_markers
                .iter()
                .any(|marker| seen_markers.contains(marker))
            {
                continue;
            }
            seen_markers.extend(scope.dsl_markers.iter().cloned());
            let members = self.members_of_type(file, reference, name, &scope.receiver_type);
            if !members.is_empty() {
                return Some(self.accessible(file, reference, members));
            }
            let extensions = self.extensions_for_type(file, reference, name, &scope.receiver_type);
            if !extensions.is_empty() {
                return Some(self.accessible(file, reference, extensions));
            }
        }
        None
    }
    pub(super) fn active_receivers(
        &self,
        file: usize,
        reference: &Reference,
    ) -> Vec<ReceiverScope> {
        let Some(mut scopes) = self.lexical_receivers(file, reference) else {
            return Vec::new();
        };
        scopes.extend(self.inferred_lambda_receivers(file, reference));
        scopes.sort_by_key(|scope| scope.end_byte.saturating_sub(scope.start_byte));
        scopes
    }
    pub(super) fn dsl_markers_for_type(
        &self,
        file: usize,
        reference: &Reference,
        type_name: &str,
    ) -> Vec<String> {
        let type_name = normalize_type(type_name);
        let final_name = type_name.rsplit('.').next().unwrap_or(&type_name);
        let types = self.resolve_declared_type(
            file,
            reference.context_id,
            &type_name,
            self.semantic_package(file, reference.context_id),
        );
        types
            .into_iter()
            .filter(|candidate| candidate.name == final_name)
            .filter_map(|candidate| candidate.declaration)
            .flat_map(|declaration| &declaration.dsl_markers)
            .filter(|annotation| {
                let marker_name = annotation.rsplit('.').next().unwrap_or(annotation);
                self.named(marker_name).any(|candidate| {
                    candidate.declaration.is_some_and(|declaration| {
                        declaration
                            .dsl_markers
                            .iter()
                            .any(|marker| marker.rsplit('.').next() == Some("DslMarker"))
                    })
                })
            })
            .cloned()
            .collect()
    }
    pub(super) fn receiver_from_function_type(
        &self,
        file: usize,
        context_id: usize,
        function_type: &str,
        package: &[String],
        seen: &mut BTreeSet<String>,
    ) -> Option<String> {
        let normalized = normalize_type(function_type);
        if !seen.insert(normalized.clone()) {
            return None;
        }
        if let Some(receiver) = function_type_receiver(&normalized) {
            let canonical = self.canonical_type_names(
                file,
                context_id,
                package,
                receiver,
                &mut BTreeSet::new(),
            );
            return only(canonical);
        }
        self.resolve_declared_type(file, context_id, &normalized, package)
            .into_iter()
            .find_map(|candidate| {
                let alias = candidate.declaration?.alias_target.as_deref()?;
                self.receiver_from_function_type(
                    candidate.file,
                    candidate.context_id,
                    alias,
                    candidate.package,
                    seen,
                )
            })
    }
    pub(super) fn callable_candidates(&self, file: usize, reference: &Reference) -> Vec<usize> {
        let package = self.semantic_package(file, reference.context_id);
        let Some(name) = reference.path.last() else {
            return Vec::new();
        };
        let candidates = if reference.path.len() == 1 {
            let explicit = self.imported_candidates(file, reference, name, false);
            if !explicit.is_empty() {
                explicit
            } else {
                let package_candidates = self
                    .named(name)
                    .filter(|candidate| candidate.package == package && candidate.owner.is_empty())
                    .filter(|candidate| self.visible_in_source_set(file, candidate))
                    .map(|candidate| candidate.candidate_index)
                    .collect::<Vec<_>>();
                if package_candidates.is_empty() {
                    self.imported_candidates(file, reference, name, true)
                } else {
                    package_candidates
                }
            }
        } else {
            self.qualified_candidates(file, reference, &reference.path, package)
        };
        self.filter_callable_overloads(file, reference, candidates)
    }
    pub(super) fn method_receiver_type(
        &self,
        file: usize,
        reference: &Reference,
        is_super: bool,
    ) -> Option<String> {
        let containing_call = self.containing_call_facts(file, reference);
        if containing_call
            .is_some_and(|call| call.convention.is_some() && call.receiver_type.is_none())
        {
            return None;
        }
        if let Some(ty) = containing_call.and_then(|call| call.receiver_type.clone()) {
            return Some(ty);
        }
        if is_super {
            return self.super_receiver_type(file, reference);
        }
        if let Some(receiver) = reference.path.first()
            && (receiver == "this" || receiver.starts_with("this@"))
        {
            return self.this_receiver_type(file, reference, receiver);
        }
        let receiver = reference.path.first()?;
        if self
            .graph
            .has_local_binding(file, receiver, reference.context_id, reference.start_byte)
            || receiver == "it"
        {
            return self
                .graph
                .binding_type(file, receiver, reference.context_id, reference.start_byte)
                .or_else(|| self.inferred_lambda_parameter_type(file, reference, receiver));
        }
        let package = self.semantic_package(file, reference.context_id);
        self.imported_type(file, reference, receiver)
            .or_else(|| self.same_package_type(package, receiver))
    }
    pub(super) fn containing_call_facts(
        &self,
        file: usize,
        reference: &Reference,
    ) -> Option<&CallFacts> {
        self.graph
            .call_facts(
                file,
                reference.context_id,
                Some(ResolutionFamily::KotlinModules),
            )
            .filter(|call| {
                call.start_byte <= reference.start_byte && reference.end_byte <= call.end_byte
            })
            .min_by_key(|call| (call.end_byte - call.start_byte, call.convention.is_none()))
    }
    pub(super) fn semantic_package(&self, file: usize, context_id: usize) -> &[String] {
        self.graph
            .semantic(file, context_id, Some(ResolutionFamily::KotlinModules))
            .find_map(|semantic| semantic.semantic.context.as_ref())
            .map_or(&[], |context| context.package.as_slice())
    }
    pub(super) fn super_receiver_type(&self, file: usize, reference: &Reference) -> Option<String> {
        let class = self.nearest_enclosing_declaration(
            file,
            reference.start_byte,
            &[DeclarationKind::Class],
        )?;
        let parents = self.direct_supertypes(class);
        match parents.as_slice() {
            [parent] => Some(candidate_type_name(parent)),
            _ => None,
        }
    }
    pub(super) fn this_receiver_type(
        &self,
        file: usize,
        reference: &Reference,
        receiver: &str,
    ) -> Option<String> {
        if let Some(label) = receiver.strip_prefix("this@") {
            if let Some(scope) = self
                .active_receivers(file, reference)
                .into_iter()
                .find(|scope| scope.label.as_deref() == Some(label))
            {
                return Some(scope.receiver_type);
            }
            let types = self
                .named(label)
                .filter(|candidate| {
                    candidate.file == file
                        && candidate.package == self.semantic_package(file, reference.context_id)
                        && is_type_declaration(candidate)
                })
                .map(candidate_type_name)
                .collect::<BTreeSet<_>>();
            return only(types);
        }
        self.nearest_enclosing_declaration(
            file,
            reference.start_byte,
            &[
                DeclarationKind::Class,
                DeclarationKind::Object,
                DeclarationKind::CompanionObject,
            ],
        )
        .map(candidate_type_name)
    }
}
