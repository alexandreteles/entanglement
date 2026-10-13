use super::*;

impl Index<'_> {
    pub(super) fn callable_candidates(&self, file: usize, reference: &Reference) -> Vec<usize> {
        let semantic = self.graph.semantic(
            file,
            reference.context_id,
            Some(ResolutionFamily::KotlinModules),
        );
        let package = semantic
            .and_then(|semantic| semantic.context.as_ref())
            .map_or(&[][..], |ctx| ctx.package.as_slice());
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
    pub(super) fn inside_unresolved_lambda_argument(
        &self,
        file: usize,
        reference: &Reference,
    ) -> bool {
        let active = self
            .graph
            .call_facts(
                file,
                reference.context_id,
                Some(ResolutionFamily::KotlinModules),
            )
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
            .min_by_key(|(_, argument)| argument.end_byte - argument.start_byte);
        let Some((call, argument)) = active else {
            return false;
        };
        if argument.lambda_receiver_type.is_some()
            || !self.inferred_lambda_receivers(file, reference).is_empty()
        {
            return false;
        }
        let Some(outer) = self.callee_reference_for_call(file, reference.context_id, call) else {
            return true;
        };
        let candidates = self.callable_candidates(file, outer);
        if candidates.is_empty() {
            return true;
        }
        let mut found_signature = false;
        let all_plain_callbacks = candidates.into_iter().all(|index| {
            let Some(signature) = self.candidates[index]
                .declaration
                .as_ref()
                .and_then(|declaration| declaration.signature.as_ref())
            else {
                return false;
            };
            let Some(parameter) =
                lambda_parameter_for_argument(signature, &call.arguments, argument)
            else {
                return false;
            };
            let Some(type_name) = parameter.type_name.as_deref() else {
                return false;
            };
            if !type_name.contains("->") {
                return false;
            }
            found_signature = true;
            function_type_receiver(type_name).is_none()
        });
        !found_signature || !all_plain_callbacks
    }
    pub(super) fn inferred_lambda_parameter_type(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
    ) -> Option<String> {
        let (call, argument) = self
            .graph
            .call_facts(
                file,
                reference.context_id,
                Some(ResolutionFamily::KotlinModules),
            )
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
            .min_by_key(|(_, argument)| argument.end_byte - argument.start_byte)?;
        let outer = self.callee_reference_for_call(file, reference.context_id, call)?;
        let callback_candidates = self.callable_candidates(file, outer);
        let mut inferred = BTreeSet::new();
        for candidate_index in callback_candidates {
            let candidate = &self.candidates[candidate_index];
            let signature = candidate
                .declaration
                .as_ref()
                .and_then(|declaration| declaration.signature.as_ref())?;
            let parameter = lambda_parameter_for_argument(signature, &call.arguments, argument)?;
            let type_name = parameter.type_name.as_deref()?;
            let inferred_arguments = self.infer_argument_return_types(file, outer, &call.arguments);
            let substitutions = generic_type_substitutions(signature, &inferred_arguments);
            let type_name = substitute_type_name(type_name, &substitutions);
            let parameters = function_type_parameters(&type_name)?;
            let parameter_index = if !argument.lambda_has_explicit_parameters {
                if name != "it" || parameters.len() != 1 {
                    return None;
                }
                0
            } else {
                argument
                    .lambda_parameter_names
                    .iter()
                    .position(|parameter_name| parameter_name == name)?
            };
            let parameter_type = parameters.get(parameter_index)?;
            let canonical = self.canonical_type_names(
                candidate.file,
                candidate.context_id,
                &candidate.package,
                parameter_type,
                &mut BTreeSet::new(),
            );
            if canonical.len() != 1 {
                return None;
            }
            inferred.insert(canonical.into_iter().next()?);
        }
        if inferred.len() != 1 {
            return None;
        }
        inferred.into_iter().next()
    }
}
