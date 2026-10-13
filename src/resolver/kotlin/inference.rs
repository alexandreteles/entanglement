use super::*;

impl Index<'_> {
    pub(super) fn callee_reference_for_call<'b>(
        &'b self,
        file: usize,
        context_id: usize,
        call: &crate::graph::CallFacts,
    ) -> Option<&'b Reference> {
        let target = call.target.as_ref()?;
        self.graph.file(file).references.iter().find(|reference| {
            reference.context_id == context_id
                && reference.start_byte == target.start_byte
                && reference.end_byte == target.end_byte
                && matches!(reference.kind, ReferenceKind::Call | ReferenceKind::Method)
        })
    }

    pub(super) fn infer_argument_return_types(
        &self,
        file: usize,
        reference: &Reference,
        arguments: &[crate::graph::CallArgumentFact],
    ) -> Vec<crate::graph::CallArgumentFact> {
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
        argument: &crate::graph::CallArgumentFact,
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
        argument: &crate::graph::CallArgumentFact,
        calls: &[crate::graph::CallFacts],
        references: &[Reference],
    ) -> Option<String> {
        let call = exact_argument_call(calls, argument)?;
        let inner = call_target_reference(references, reference.context_id, call)?;
        let candidates = self.call_candidates(file, inner);
        let [candidate] = candidates.as_slice() else {
            return None;
        };
        self.inferred_call_result_type(file, inner, call, &self.candidates[*candidate])
    }

    pub(super) fn call_candidates(&self, file: usize, reference: &Reference) -> Vec<usize> {
        let Some(name) = reference.path.last() else {
            return Vec::new();
        };
        let package = self.semantic_package(file, reference.context_id);
        let candidates = if reference.path.len() == 1 {
            self.simple_candidates(file, reference, name, package)
        } else {
            self.qualified_candidates(file, reference, &reference.path, package)
        };
        let candidates = self.filter_callable_overloads(file, reference, candidates);
        let candidates = self.select_expect_actual(file, candidates);
        self.accessible(file, reference, candidates)
    }

    pub(super) fn inferred_call_result_type(
        &self,
        file: usize,
        reference: &Reference,
        call: &crate::graph::CallFacts,
        candidate: &Candidate,
    ) -> Option<String> {
        if is_type_declaration(candidate) {
            return Some(candidate_type_name(candidate));
        }
        let signature = candidate.declaration.as_ref()?.signature.as_ref()?;
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
        let semantic = self
            .graph
            .semantic(file, context_id, Some(ResolutionFamily::KotlinModules));
        let package = semantic
            .and_then(|semantic| semantic.context.as_ref())
            .map_or(&[][..], |semantic| semantic.package.as_slice());
        let canonical =
            self.canonical_type_names(file, context_id, package, type_name, &mut BTreeSet::new());
        (canonical.len() == 1)
            .then(|| canonical.into_iter().next())
            .flatten()
    }
}
