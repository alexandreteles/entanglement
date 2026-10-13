use super::*;

impl Index<'_> {
    pub(super) fn extensions_for_type(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
        type_name: &str,
    ) -> Vec<usize> {
        let semantic = self.graph.semantic(
            file,
            reference.context_id,
            Some(ResolutionFamily::KotlinModules),
        );
        let current_package = semantic
            .and_then(|item| item.context.as_ref())
            .map_or(&[][..], |item| item.package.as_slice());
        let expected = self.canonical_type_names(
            file,
            reference.context_id,
            current_package,
            type_name,
            &mut BTreeSet::new(),
        );
        let imports = self.imports(file, reference.context_id).collect::<Vec<_>>();
        self.named(name)
            .filter(|candidate| candidate.owner.is_empty())
            .filter(|candidate| is_extension_candidate(candidate))
            .filter(|candidate| self.visible_in_source_set(file, candidate))
            .filter(|candidate| {
                candidate.package == current_package
                    || imports.iter().any(|import| {
                        if import.namespace {
                            import.path == candidate.package
                        } else {
                            import
                                .path
                                .last()
                                .is_some_and(|imported_name| imported_name == name)
                                && import.path[..import.path.len().saturating_sub(1)]
                                    == candidate.package
                        }
                    })
            })
            .filter(|candidate| {
                let Some(receiver) = candidate
                    .declaration
                    .as_ref()
                    .and_then(|item| item.signature.as_ref())
                    .and_then(|signature| signature.receiver_type.as_deref())
                else {
                    return false;
                };
                let actual = self.canonical_type_names(
                    candidate.file,
                    candidate.context_id,
                    &candidate.package,
                    receiver,
                    &mut BTreeSet::new(),
                );
                !expected.is_empty()
                    && !actual.is_empty()
                    && expected.iter().any(|item| actual.contains(item))
            })
            .map(|candidate| candidate.candidate_index)
            .collect()
    }
}
