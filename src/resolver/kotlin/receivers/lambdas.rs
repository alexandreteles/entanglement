use super::*;

impl Index<'_> {
    pub(in crate::resolver::kotlin) fn active_lambda_arguments<'b>(
        &'b self,
        file: usize,
        reference: &Reference,
    ) -> impl Iterator<Item = (&'b CallFacts, &'b CallArgumentFact)> + 'b {
        let position = reference.start_byte;
        self.graph
            .call_facts(
                file,
                reference.context_id,
                Some(ResolutionFamily::KotlinModules),
            )
            .filter_map(move |call| {
                call.arguments
                    .iter()
                    .find(|argument| {
                        argument.is_lambda
                            && argument.start_byte <= position
                            && position < argument.end_byte
                    })
                    .map(|argument| (call, argument))
            })
    }

    pub(in crate::resolver::kotlin) fn inferred_lambda_receivers(
        &self,
        file: usize,
        reference: &Reference,
    ) -> Vec<ReceiverScope> {
        let mut active_arguments = self
            .active_lambda_arguments(file, reference)
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

    pub(in crate::resolver::kotlin) fn inferred_receiver_for_lambda(
        &self,
        file: usize,
        reference: &Reference,
        call: &CallFacts,
        argument: &CallArgumentFact,
        inherited_receivers: &[ReceiverScope],
    ) -> Option<String> {
        let outer = self.callee_reference_for_call(file, reference.context_id, call)?;
        let mut lexical_receivers = self.lexical_receivers(file, reference).unwrap_or_default();
        lexical_receivers.extend_from_slice(inherited_receivers);
        lexical_receivers.sort_by_key(|scope| scope.end_byte.saturating_sub(scope.start_byte));
        let candidates = if outer.path.len() == 1 {
            self.implicit_receiver_candidates_from_scopes(
                file,
                outer,
                outer.path.last()?,
                lexical_receivers,
            )
            .map(|candidates| self.filter_callable_overloads(file, outer, candidates))
            .unwrap_or_else(|| self.callable_candidates(file, outer))
        } else {
            self.callable_candidates(file, outer)
        };
        let arguments = self.call_arguments(file, outer.context_id, call);
        let receivers = candidates
            .into_iter()
            .filter_map(|index| {
                let candidate = &self.candidates[index];
                let type_name =
                    self.lambda_argument_type(file, outer, &arguments, argument, candidate)?;
                self.receiver_from_function_type(
                    self.candidates[index].file,
                    self.candidates[index].context_id,
                    &type_name,
                    self.candidates[index].package,
                    &mut BTreeSet::new(),
                )
            })
            .collect::<BTreeSet<_>>();
        only(receivers)
    }

    pub(in crate::resolver::kotlin) fn lambda_argument_type(
        &self,
        file: usize,
        reference: &Reference,
        arguments: &CallArguments<'_>,
        argument: &CallArgumentFact,
        candidate: &Candidate,
    ) -> Option<String> {
        let signature = candidate.signature()?;
        let parameter = arguments.bind(signature).lambda_parameter(argument)?;
        let type_name = parameter.type_name.as_deref()?;
        let inferred = self.infer_argument_return_types(file, reference, arguments);
        let typed = arguments.with_inferred(&inferred);
        Some(substitute_type_name(
            type_name,
            typed.bind(signature).substitutions(),
        ))
    }

    pub(in crate::resolver::kotlin) fn inside_unresolved_lambda_argument(
        &self,
        file: usize,
        reference: &Reference,
    ) -> bool {
        let active = self
            .active_lambda_arguments(file, reference)
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
        let arguments = self.call_arguments(file, outer.context_id, call);
        let all_plain_callbacks = candidates.into_iter().all(|index| {
            let Some(signature) = self.candidates[index].signature() else {
                return false;
            };
            let Some(parameter) = arguments.bind(signature).lambda_parameter(argument) else {
                return false;
            };
            let Some(type_name) = parameter.type_name.as_deref() else {
                return false;
            };
            if !type_name.contains("->") {
                return false;
            }
            function_type_receiver(type_name).is_none()
        });
        !all_plain_callbacks
    }

    pub(in crate::resolver::kotlin) fn inferred_lambda_parameter_type(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
    ) -> Option<String> {
        let (call, argument) = self
            .active_lambda_arguments(file, reference)
            .min_by_key(|(_, argument)| argument.end_byte - argument.start_byte)?;
        let outer = self.callee_reference_for_call(file, reference.context_id, call)?;
        let callback_candidates = self.callable_candidates(file, outer);
        let mut inferred = BTreeSet::new();
        let arguments = self.call_arguments(file, outer.context_id, call);
        for candidate_index in callback_candidates {
            let candidate = &self.candidates[candidate_index];
            let type_name =
                self.lambda_argument_type(file, outer, &arguments, argument, candidate)?;
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
                candidate.package,
                parameter_type,
                &mut BTreeSet::new(),
            );
            inferred.insert(only(canonical)?);
        }
        only(inferred)
    }
}
