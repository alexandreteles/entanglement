use super::*;

impl Index<'_> {
    pub(super) fn callee_reference_for_call<'b>(
        &'b self,
        file: usize,
        context_id: usize,
        call: &CallFacts,
    ) -> Option<&'b Reference> {
        call_target_reference(&self.graph.file(file).references, context_id, call)
    }

    pub(super) fn infer_argument_return_types(
        &self,
        file: usize,
        reference: &Reference,
        arguments: &[CallArgumentFact],
    ) -> Vec<CallArgumentFact> {
        let mut inferred = arguments.to_vec();
        let source = self
            .snapshot
            .read(&self.graph.file(file).target)
            .ok()
            .flatten();
        let calls = self.graph.call_facts(
            file,
            reference.context_id,
            Some(ResolutionFamily::KotlinModules),
        );
        let references = &self.graph.file(file).references;
        for argument in inferred
            .iter_mut()
            .filter(|argument| !argument.is_lambda && argument.type_name.is_none())
        {
            argument.type_name = self
                .infer_identifier_argument(file, reference, argument, source.as_deref())
                .or_else(|| {
                    self.infer_call_argument_type(file, reference, argument, calls, references)
                });
        }
        inferred
    }

    pub(super) fn infer_identifier_argument(
        &self,
        file: usize,
        reference: &Reference,
        argument: &CallArgumentFact,
        source: Option<&[u8]>,
    ) -> Option<String> {
        let expression = source?.get(argument.start_byte..argument.end_byte)?;
        let expression = std::str::from_utf8(expression)
            .ok()?
            .trim()
            .trim_matches('`');
        if expression.is_empty()
            || !expression
                .chars()
                .all(|character| character.is_alphanumeric() || character == '_')
        {
            return None;
        }
        let type_name =
            self.graph
                .binding_type(file, expression, reference.context_id, argument.start_byte)?;
        Some(
            self.canonicalize_type_in_context(file, reference.context_id, &type_name)
                .unwrap_or(type_name),
        )
    }

    pub(super) fn infer_call_argument_type(
        &self,
        file: usize,
        reference: &Reference,
        argument: &CallArgumentFact,
        calls: &[CallFacts],
        references: &[Reference],
    ) -> Option<String> {
        let call = exact_argument_call(calls, argument)?;
        let inner = call_target_reference(references, reference.context_id, call)?;
        let candidates = self.reference_candidates(file, inner);
        let [candidate] = candidates.as_slice() else {
            return None;
        };
        self.inferred_call_result_type(file, inner, call, &self.candidates[*candidate])
    }

    pub(super) fn reference_candidates(&self, file: usize, reference: &Reference) -> Vec<usize> {
        let Some(name) = reference.path.last() else {
            return Vec::new();
        };
        let package = self.semantic_package(file, reference.context_id);
        let candidates = match reference.path.as_slice() {
            [_] => self.simple_candidates(file, reference, name, package),
            _ => self.qualified_candidates(file, reference, &reference.path, package),
        };
        let candidates = self.filter_callable_overloads(file, reference, candidates);
        self.select_expect_actual(file, candidates)
    }

    pub(super) fn inferred_call_result_type(
        &self,
        file: usize,
        reference: &Reference,
        call: &CallFacts,
        candidate: &Candidate,
    ) -> Option<String> {
        if is_type_declaration(candidate) {
            return Some(candidate_type_name(candidate));
        }
        let signature = candidate.signature()?;
        let inferred_arguments = self.infer_argument_return_types(file, reference, &call.arguments);
        let substitutions = generic_type_substitutions(signature, &inferred_arguments);
        let return_type = substitute_type_name(signature.return_type.as_deref()?, &substitutions);
        Some(
            self.canonicalize_type_in_context(candidate.file, candidate.context_id, &return_type)
                .unwrap_or(return_type),
        )
    }

    pub(super) fn canonicalize_type_in_context(
        &self,
        file: usize,
        context_id: usize,
        type_name: &str,
    ) -> Option<String> {
        let package = self.semantic_package(file, context_id);
        let canonical =
            self.canonical_type_names(file, context_id, package, type_name, &mut BTreeSet::new());
        (canonical.len() == 1)
            .then(|| canonical.into_iter().next())
            .flatten()
    }
    pub(super) fn filter_callable_overloads(
        &self,
        file: usize,
        reference: &Reference,
        mut candidates: Vec<usize>,
    ) -> Vec<usize> {
        candidates.sort_unstable();
        candidates.dedup();
        if !matches!(reference.kind, ReferenceKind::Call | ReferenceKind::Method) {
            return candidates;
        }
        let Some(call) = self.containing_call_facts(file, reference) else {
            return candidates;
        };
        let arguments = self.infer_argument_return_types(file, reference, &call.arguments);
        candidates
            .iter()
            .copied()
            .filter(|index| {
                let candidate = &self.candidates[*index];
                if !matches_convention(candidate, call.convention) {
                    return false;
                }
                if reference.kind == ReferenceKind::Call && is_type_declaration(candidate) {
                    return self.type_constructor_matches(candidate, &arguments);
                }
                let Some(signature) = self.candidates[*index].signature() else {
                    return true;
                };
                arguments_match(signature, &arguments)
            })
            .collect::<Vec<_>>()
    }

    pub(super) fn select_expect_actual(&self, file: usize, candidates: Vec<usize>) -> Vec<usize> {
        if candidates.len() < 2 {
            return candidates;
        }
        let caller = &self.sources[file];
        let has_platform = caller
            .source_set
            .as_deref()
            .is_some_and(|set| !set.starts_with("common"));
        if !has_platform {
            return candidates
                .into_iter()
                .filter(|candidate| {
                    let declaration = self.candidates[*candidate].declaration;
                    declaration.is_none_or(|declaration| !declaration.modifiers.is_actual)
                })
                .collect();
        }
        let actual = candidates
            .iter()
            .copied()
            .filter(|index| {
                self.candidates[*index]
                    .declaration
                    .is_some_and(|item| item.modifiers.is_actual)
            })
            .collect::<Vec<_>>();
        if actual.is_empty() {
            return candidates;
        }
        candidates
            .into_iter()
            .filter(|index| {
                let Some(declaration) = self.candidates[*index].declaration else {
                    return true;
                };
                if !declaration.modifiers.is_expect {
                    return true;
                }
                !actual.iter().any(|actual_index| {
                    same_logical_declaration(
                        &self.candidates[*index],
                        &self.candidates[*actual_index],
                    )
                })
            })
            .collect()
    }
}
