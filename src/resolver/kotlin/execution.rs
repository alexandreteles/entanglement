use super::*;

impl Index<'_> {
    pub(super) fn finish_resolution(
        &self,
        file: usize,
        reference: &Reference,
        candidates: Vec<usize>,
    ) -> ReferenceAnalysis {
        let candidates = unique_candidates(candidates);
        let name = reference
            .path
            .last()
            .map(String::as_str)
            .unwrap_or_default();
        match candidates.as_slice() {
            [] if is_kotlin_builtin(name, reference) => {
                analysis(reference, Resolution::External, None)
            }
            [] => analysis(reference, Resolution::Unresolved, None),
            [candidate] => {
                let candidate = &self.candidates[*candidate];
                let accessor_access = matches!(
                    reference.kind,
                    ReferenceKind::Value | ReferenceKind::Qualified
                ) && matches!(
                    reference.access,
                    Some(
                        ReferenceAccess::Read | ReferenceAccess::Write | ReferenceAccess::ReadWrite
                    )
                ) && candidate
                    .declaration
                    .is_some_and(|declaration| declaration.kind == DeclarationKind::Property);
                let dispatch =
                    if matches!(reference.kind, ReferenceKind::Call | ReferenceKind::Method)
                        || accessor_access
                    {
                        Some(call_dispatch(candidate, reference))
                    } else {
                        None
                    };
                let mut result = analysis(
                    reference,
                    Resolution::Exact((*candidate.symbol).clone()),
                    dispatch,
                );
                let ranges = self.execution_ranges_for_reference(file, reference, candidate);
                if !ranges.is_empty()
                    && let Some(caller_start_byte) =
                        reference.execution_owner.or(reference.call_owner)
                    && dispatch == Some(CallDispatch::Direct)
                {
                    result
                        .execution_targets
                        .extend(ranges.into_iter().map(|range| ExecutionTarget {
                            caller_start_byte,
                            file_index: candidate.file,
                            context_id: candidate.context_id,
                            range,
                        }));
                }
                result
            }
            _ => analysis(
                reference,
                Resolution::Ambiguous(
                    candidates
                        .into_iter()
                        .map(|candidate| (*self.candidates[candidate].symbol).clone())
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                ),
                None,
            ),
        }
    }

    pub(super) fn execution_ranges_for_reference(
        &self,
        file: usize,
        reference: &Reference,
        candidate: &Candidate,
    ) -> Vec<SourceRange> {
        let Some(declaration) = candidate.declaration else {
            return Vec::new();
        };
        let Some(execution) = declaration.execution.as_ref() else {
            return Vec::new();
        };
        match (reference.kind, declaration.kind) {
            (ReferenceKind::Call, DeclarationKind::Property) => execution
                .callable_value
                .or(execution.getter)
                .into_iter()
                .collect(),
            (ReferenceKind::Call, _) if is_type_declaration(candidate) => {
                self.constructor_execution_ranges(file, reference, candidate, execution)
            }
            (ReferenceKind::Value | ReferenceKind::Qualified, DeclarationKind::Property) => {
                self.accessor_execution_ranges(reference.access, execution)
            }
            _ => Vec::new(),
        }
    }

    pub(super) fn accessor_execution_ranges(
        &self,
        access: Option<ReferenceAccess>,
        execution: &DeclarationExecutionFacts,
    ) -> Vec<SourceRange> {
        match access {
            Some(ReferenceAccess::Write) => execution.setter.into_iter().collect(),
            Some(ReferenceAccess::ReadWrite) => execution
                .getter
                .into_iter()
                .chain(execution.setter)
                .collect(),
            Some(ReferenceAccess::Read) => execution.getter.into_iter().collect(),
            None => Vec::new(),
        }
    }
}
