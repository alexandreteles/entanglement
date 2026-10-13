use super::*;

impl Index<'_> {
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
        let Some(call) = self
            .graph
            .call_facts(
                file,
                reference.context_id,
                Some(ResolutionFamily::KotlinModules),
            )
            .iter()
            .filter(|call| {
                call.start_byte <= reference.start_byte && reference.end_byte <= call.end_byte
            })
            .min_by_key(|call| (call.end_byte - call.start_byte, call.convention.is_none()))
        else {
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
                let Some(signature) = self.candidates[*index]
                    .declaration
                    .as_ref()
                    .and_then(|item| item.signature.as_ref())
                else {
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
                    let declaration = self.candidates[*candidate].declaration.as_ref();
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
                    .as_ref()
                    .is_some_and(|item| item.modifiers.is_actual)
            })
            .collect::<Vec<_>>();
        if actual.is_empty() {
            return candidates;
        }
        candidates
            .into_iter()
            .filter(|index| {
                let Some(declaration) = self.candidates[*index].declaration.as_ref() else {
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
