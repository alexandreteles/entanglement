use super::*;

impl Index<'_> {
    pub(super) fn constructor_execution_ranges(
        &self,
        file: usize,
        reference: &Reference,
        candidate: &Candidate,
        execution: &DeclarationExecutionFacts,
    ) -> Vec<SourceRange> {
        let constructor_class = self.typealias_target(candidate).unwrap_or(candidate);
        let constructor_execution = constructor_class
            .declaration
            .and_then(|declaration| declaration.execution.as_ref());
        if constructor_execution.is_none() && candidate.signature().is_none() {
            return Vec::new();
        }
        let Some(call) = self.containing_call_facts(file, reference) else {
            return Vec::new();
        };
        let arguments = self.call_arguments(file, reference.context_id, call);
        let inferred = self.infer_argument_return_types(file, reference, &arguments);
        let arguments = arguments.with_inferred(&inferred);
        if let Some(range) = constructor_execution.and_then(|execution| {
            self.selected_constructor_range(constructor_class, execution, &arguments)
        }) {
            return vec![range];
        }
        if execution.constructors.is_empty()
            && candidate
                .signature()
                .is_some_and(|signature| arguments.bind(signature).matches())
        {
            execution.initializers.clone()
        } else {
            Vec::new()
        }
    }

    pub(super) fn selected_constructor_range(
        &self,
        class: &Candidate,
        execution: &DeclarationExecutionFacts,
        arguments: &CallArguments<'_>,
    ) -> Option<SourceRange> {
        let mut matched =
            self.primary_constructor_matches(class, &execution.constructors, arguments);
        matched.extend(self.secondary_constructor_matches(
            class,
            &execution.constructors,
            arguments,
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
        arguments: &CallArguments<'_>,
    ) -> Vec<SourceRange> {
        let matches_signature = class
            .signature()
            .is_some_and(|signature| arguments.bind(signature).matches());
        if !matches_signature {
            return Vec::new();
        }
        let owner = extended_owner(class.owner, class.name);
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
                    .is_some_and(|declaration| declaration.kind == DeclarationKind::Constructor)
        })
    }

    pub(super) fn secondary_constructor_matches(
        &self,
        class: &Candidate,
        ranges: &[SourceRange],
        arguments: &CallArguments<'_>,
    ) -> Vec<SourceRange> {
        let owner = extended_owner(class.owner, class.name);
        self.candidates
            .iter()
            .filter(|candidate| {
                candidate.file == class.file
                    && candidate.context_id == class.context_id
                    && candidate.owner == owner
                    && candidate
                        .declaration
                        .is_some_and(|declaration| declaration.kind == DeclarationKind::Constructor)
            })
            .filter(|candidate| {
                candidate
                    .signature()
                    .is_some_and(|signature| arguments.bind(signature).matches())
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
        candidate: &'b Candidate<'b>,
    ) -> Option<&'b Candidate<'b>> {
        let declaration = candidate.declaration?;
        if declaration.kind != DeclarationKind::TypeAlias {
            return None;
        }
        let alias = declaration.alias_target.as_deref()?;
        let mut targets = self.resolve_declared_type(
            candidate.file,
            candidate.context_id,
            alias,
            candidate.package,
        );
        if targets.len() != 1 {
            return None;
        }
        targets.pop()
    }

    pub(super) fn type_constructor_matches(
        &self,
        candidate: &Candidate,
        arguments: &CallArguments<'_>,
    ) -> bool {
        let target = self.typealias_target(candidate).unwrap_or(candidate);
        if target
            .signature()
            .is_some_and(|signature| arguments.bind(signature).matches())
        {
            return true;
        }
        let owner = extended_owner(target.owner, target.name);
        self.candidates.iter().any(|constructor| {
            constructor.file == target.file
                && constructor.context_id == target.context_id
                && constructor.owner == owner
                && constructor.declaration.is_some_and(|declaration| {
                    declaration.kind == DeclarationKind::Constructor
                        && declaration
                            .signature
                            .as_ref()
                            .is_some_and(|signature| arguments.bind(signature).matches())
                })
        })
    }
}
