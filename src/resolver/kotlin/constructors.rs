use super::*;

impl Index<'_> {
    pub(super) fn matches_primary_constructor(
        &self,
        file: usize,
        reference: &Reference,
        candidate: &Candidate,
    ) -> bool {
        let Some(declaration) = candidate.declaration.as_ref() else {
            return false;
        };
        let Some(signature) = declaration.signature.as_ref() else {
            return false;
        };
        let Some(call) = self.containing_call_facts(file, reference) else {
            return false;
        };
        let arguments = self.infer_argument_return_types(file, reference, &call.arguments);
        arguments_match(signature, &arguments)
            && declaration
                .execution
                .as_ref()
                .is_some_and(|execution| execution.constructors.is_empty())
    }

    pub(super) fn selected_constructor_range(
        &self,
        file: usize,
        reference: &Reference,
        class: &Candidate,
    ) -> Option<SourceRange> {
        let constructor_class = self.typealias_target(class).unwrap_or(class);
        let execution = constructor_class.declaration.as_ref()?.execution.as_ref()?;
        let call = self.containing_call_facts(file, reference)?;
        let arguments = self.infer_argument_return_types(file, reference, &call.arguments);
        let mut matched = self.primary_constructor_matches(
            constructor_class,
            &execution.constructors,
            &arguments,
        );
        matched.extend(self.secondary_constructor_matches(
            constructor_class,
            &execution.constructors,
            &arguments,
        ));
        matched.sort_by_key(|range| (range.start_byte, range.end_byte));
        matched.dedup();
        if matched.len() == 1 {
            matched.pop()
        } else {
            None
        }
    }

    pub(super) fn primary_constructor_matches(
        &self,
        class: &Candidate,
        ranges: &[SourceRange],
        arguments: &[CallArgumentFact],
    ) -> Vec<SourceRange> {
        let matches_signature = class
            .signature()
            .is_some_and(|signature| arguments_match(signature, arguments));
        if !matches_signature {
            return Vec::new();
        }
        let owner = extended_owner(&class.owner, &class.name);
        ranges
            .iter()
            .copied()
            .filter(|range| !self.is_secondary_constructor_range(class, &owner, *range))
            .take(1)
            .collect()
    }

    pub(super) fn is_secondary_constructor_range(
        &self,
        class: &Candidate,
        owner: &[String],
        range: SourceRange,
    ) -> bool {
        self.candidates.iter().any(|candidate| {
            candidate.file == class.file
                && candidate.context_id == class.context_id
                && candidate.owner == owner
                && candidate.start == range.start_byte
                && candidate.end == range.end_byte
                && candidate
                    .declaration
                    .as_ref()
                    .is_some_and(|declaration| declaration.kind == DeclarationKind::Constructor)
        })
    }

    pub(super) fn secondary_constructor_matches(
        &self,
        class: &Candidate,
        ranges: &[SourceRange],
        arguments: &[CallArgumentFact],
    ) -> Vec<SourceRange> {
        let owner = extended_owner(&class.owner, &class.name);
        self.candidates
            .iter()
            .filter(|candidate| {
                candidate.file == class.file
                    && candidate.context_id == class.context_id
                    && candidate.owner == owner
                    && candidate
                        .declaration
                        .as_ref()
                        .is_some_and(|declaration| declaration.kind == DeclarationKind::Constructor)
            })
            .filter(|candidate| {
                candidate
                    .signature()
                    .is_some_and(|signature| arguments_match(signature, arguments))
            })
            .filter_map(|candidate| {
                ranges.iter().copied().find(|range| {
                    range.start_byte == candidate.start && range.end_byte == candidate.end
                })
            })
            .collect()
    }

    pub(super) fn typealias_target<'b>(
        &'b self,
        candidate: &'b Candidate,
    ) -> Option<&'b Candidate> {
        let declaration = candidate.declaration.as_ref()?;
        if declaration.kind != DeclarationKind::TypeAlias {
            return None;
        }
        let alias = declaration.alias_target.as_deref()?;
        let mut targets = self.resolve_declared_type(
            candidate.file,
            candidate.context_id,
            alias,
            &candidate.package,
        );
        if targets.len() != 1 {
            return None;
        }
        targets.pop()
    }

    pub(super) fn type_constructor_matches(
        &self,
        candidate: &Candidate,
        arguments: &[CallArgumentFact],
    ) -> bool {
        let target = self.typealias_target(candidate).unwrap_or(candidate);
        if target
            .signature()
            .is_some_and(|signature| arguments_match(signature, arguments))
        {
            return true;
        }
        let mut owner = target.owner.clone();
        owner.push(target.name.clone());
        self.candidates.iter().any(|constructor| {
            constructor.file == target.file
                && constructor.context_id == target.context_id
                && constructor.owner == owner
                && constructor.declaration.as_ref().is_some_and(|declaration| {
                    declaration.kind == DeclarationKind::Constructor
                        && declaration
                            .signature
                            .as_ref()
                            .is_some_and(|signature| arguments_match(signature, arguments))
                })
        })
    }
}
