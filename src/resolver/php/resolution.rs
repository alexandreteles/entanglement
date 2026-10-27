use super::*;

impl Index<'_> {
    pub(super) fn resolve(&self, file: usize, reference: &Reference) -> ReferenceAnalysis {
        let Some(php) = self.facts(file, reference.context_id).and_then(|facts| {
            facts
                .references
                .iter()
                .find(|item| item.start_byte == reference.start_byte)
        }) else {
            return analysis(reference, Resolution::Unresolved, None);
        };
        let members = match php.kind {
            PhpReferenceKind::Method
            | PhpReferenceKind::StaticMethod
            | PhpReferenceKind::Property
            | PhpReferenceKind::StaticProperty
            | PhpReferenceKind::ClassConstant => {
                self.member_reference(file, reference, php, &mut HashSet::new())
            }
            PhpReferenceKind::Variable => {
                return self.callable_reference(file, reference, php);
            }
            _ => self
                .lookup(
                    Site::at(file, reference),
                    &php.name,
                    names::reference_kind(php.kind),
                    php.class.as_deref(),
                )
                .into_iter()
                .map(Member::new)
                .collect(),
        };
        self.finish(file, reference, php, members)
    }

    pub(super) fn finish(
        &self,
        file: usize,
        reference: &Reference,
        php: &PhpReference,
        mut members: Vec<Member>,
    ) -> ReferenceAnalysis {
        let direct = members
            .iter()
            .all(|member| self.dispatch(php, member) == CallDispatch::Direct);
        members.sort_by_key(|member| member.candidate);
        members.dedup_by_key(|member| member.candidate);
        if members
            .iter()
            .any(|member| self.candidates[member.candidate].php.conditional)
        {
            return analysis(reference, Resolution::Unresolved, None);
        }
        let resolution = match members.as_slice() {
            [] => match self
                .expanded_names(
                    Site::at(file, reference),
                    &php.name,
                    names::reference_kind(php.kind),
                )
                .iter()
                .any(|name| self.projects.external(file, name))
            {
                true => Resolution::External,
                false => Resolution::Unresolved,
            },
            [member] => Resolution::Exact(self.candidates[member.candidate].symbol.clone()),
            _ => Resolution::Ambiguous(
                members
                    .iter()
                    .map(|member| self.candidates[member.candidate].symbol.clone())
                    .collect(),
            ),
        };
        let mut result = analysis(reference, resolution, None);
        if let [member] = members.as_slice() {
            let executable = php.callable
                || self.candidates[member.candidate].php.kind == PhpSymbolKind::Property;
            result.dispatch = executable.then_some(match direct {
                true => CallDispatch::Direct,
                false => CallDispatch::Virtual,
            });
            self.execution_targets(file, reference, php, member, &mut result);
        }
        result
    }
}
