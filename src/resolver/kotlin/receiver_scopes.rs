use super::*;

impl Index<'_> {
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
        let Some(semantic) = self.graph.semantic(
            file,
            reference.context_id,
            Some(ResolutionFamily::KotlinModules),
        ) else {
            return Vec::new();
        };
        let mut scopes = semantic
            .receivers
            .iter()
            .filter(|scope| {
                scope.implicit
                    && scope.start_byte <= reference.start_byte
                    && reference.start_byte < scope.end_byte
            })
            .cloned()
            .collect::<Vec<_>>();
        scopes.extend(self.inferred_lambda_receivers(file, reference));
        scopes.sort_by_key(|scope| scope.end_byte.saturating_sub(scope.start_byte));
        scopes
    }
    pub(super) fn inferred_lambda_receivers(
        &self,
        file: usize,
        reference: &Reference,
    ) -> Vec<ReceiverScope> {
        let calls = self.graph.call_facts(
            file,
            reference.context_id,
            Some(ResolutionFamily::KotlinModules),
        );
        let mut active_arguments = calls
            .iter()
            .filter_map(|call| {
                call.arguments
                    .iter()
                    .find(|argument| {
                        argument.is_lambda
                            && argument.start_byte <= reference.start_byte
                            && reference.start_byte < argument.end_byte
                    })
                    .map(|argument| (call, argument))
            })
            .collect::<Vec<_>>();
        active_arguments.sort_by_key(|(_, argument)| {
            (argument.start_byte, std::cmp::Reverse(argument.end_byte))
        });
        active_arguments.dedup_by_key(|(_, argument)| (argument.start_byte, argument.end_byte));

        let mut inferred = Vec::<ReceiverScope>::new();
        for (call, argument) in active_arguments {
            let Some(receiver_type) = argument.lambda_receiver_type.clone().or_else(|| {
                self.inferred_receiver_for_lambda(file, reference, call, argument, &inferred)
            }) else {
                continue;
            };
            let outer = self.callee_reference_for_call(file, reference.context_id, call);
            let label = argument
                .lambda_label
                .clone()
                .or_else(|| outer.and_then(|call| call.path.last().cloned()));
            inferred.push(ReceiverScope {
                start_byte: argument.start_byte,
                end_byte: argument.end_byte,
                dsl_markers: self.dsl_markers_for_type(file, reference, &receiver_type),
                receiver_type,
                implicit: true,
                label,
            });
        }
        inferred
    }
    pub(super) fn inferred_receiver_for_lambda(
        &self,
        file: usize,
        reference: &Reference,
        call: &crate::graph::CallFacts,
        argument: &crate::graph::CallArgumentFact,
        inherited_receivers: &[ReceiverScope],
    ) -> Option<String> {
        let outer = self.callee_reference_for_call(file, reference.context_id, call)?;
        let mut lexical_receivers = self
            .graph
            .semantic(
                file,
                reference.context_id,
                Some(ResolutionFamily::KotlinModules),
            )
            .into_iter()
            .flat_map(|semantic| semantic.receivers.iter())
            .filter(|scope| {
                scope.implicit
                    && scope.start_byte <= reference.start_byte
                    && reference.start_byte < scope.end_byte
            })
            .cloned()
            .collect::<Vec<_>>();
        lexical_receivers.extend_from_slice(inherited_receivers);
        lexical_receivers.sort_by_key(|scope| scope.end_byte.saturating_sub(scope.start_byte));
        let candidates = if outer.path.len() == 1 {
            self.implicit_receiver_candidates_from_scopes(
                file,
                outer,
                outer.path.last()?,
                lexical_receivers,
            )
            .unwrap_or_else(|| self.callable_candidates(file, outer))
        } else {
            self.callable_candidates(file, outer)
        };
        let candidates = self.filter_callable_overloads(file, outer, candidates);
        let mut receivers = candidates
            .into_iter()
            .filter_map(|index| {
                let signature = self.candidates[index]
                    .declaration
                    .as_ref()?
                    .signature
                    .as_ref()?;
                let parameter =
                    lambda_parameter_for_argument(signature, &call.arguments, argument)?;
                let type_name = parameter.type_name.as_deref()?;
                let inferred_arguments =
                    self.infer_argument_return_types(file, outer, &call.arguments);
                let substitutions = generic_type_substitutions(signature, &inferred_arguments);
                let type_name = substitute_type_name(type_name, &substitutions);
                self.receiver_from_function_type(
                    self.candidates[index].file,
                    self.candidates[index].context_id,
                    &type_name,
                    &self.candidates[index].package,
                    &mut BTreeSet::new(),
                )
            })
            .collect::<BTreeSet<_>>();
        (receivers.len() == 1)
            .then(|| receivers.pop_first())
            .flatten()
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
            self.graph
                .semantic(
                    file,
                    reference.context_id,
                    Some(ResolutionFamily::KotlinModules),
                )
                .and_then(|semantic| semantic.context.as_ref())
                .map_or(&[][..], |context| context.package.as_slice()),
        );
        let mut markers = Vec::new();
        for ty in types
            .into_iter()
            .filter(|candidate| candidate.name == final_name)
        {
            let Some(declaration) = ty.declaration.as_ref() else {
                continue;
            };
            for annotation in &declaration.dsl_markers {
                let marker_name = annotation.rsplit('.').next().unwrap_or(annotation);
                let is_dsl_marker = self.named(marker_name).any(|candidate| {
                    candidate.declaration.as_ref().is_some_and(|declaration| {
                        declaration
                            .dsl_markers
                            .iter()
                            .any(|marker| marker.rsplit('.').next() == Some("DslMarker"))
                    })
                });
                if is_dsl_marker {
                    markers.push(annotation.clone());
                }
            }
        }
        markers
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
            return (canonical.len() == 1)
                .then(|| canonical.into_iter().next())
                .flatten();
        }
        for candidate in self.resolve_declared_type(file, context_id, &normalized, package) {
            let Some(alias) = candidate
                .declaration
                .as_ref()
                .and_then(|declaration| declaration.alias_target.as_deref())
            else {
                continue;
            };
            if let Some(receiver) = self.receiver_from_function_type(
                candidate.file,
                candidate.context_id,
                alias,
                &candidate.package,
                seen,
            ) {
                return Some(receiver);
            }
        }
        None
    }
}
