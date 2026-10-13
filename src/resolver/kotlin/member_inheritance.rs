use super::*;

impl Index<'_> {
    pub(super) fn inherited_member_candidates(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
        classes: &[&Candidate],
        seen: &mut BTreeSet<SymbolId>,
    ) -> Vec<usize> {
        let mut inherited = Vec::new();
        for class in classes {
            if !seen.insert(class.symbol.clone()) {
                continue;
            }
            let Some(declaration) = class.declaration.as_ref() else {
                continue;
            };
            for supertype in &declaration.supertypes {
                let parents = self.resolve_declared_type(
                    class.file,
                    class.context_id,
                    supertype,
                    &class.package,
                );
                for parent in parents {
                    inherited.extend(
                        self.inherited_members_from_parent(file, reference, name, parent, seen),
                    );
                }
            }
        }
        inherited
    }
    pub(super) fn inherited_members_from_parent(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
        parent: &Candidate,
        seen: &mut BTreeSet<SymbolId>,
    ) -> Vec<usize> {
        self.members_of_type_seen(
            file,
            reference,
            name,
            &candidate_type_name(parent),
            false,
            seen,
        )
    }
    pub(super) fn merge_inherited_members(
        &self,
        members: Vec<usize>,
        mut inherited: Vec<usize>,
    ) -> Vec<usize> {
        inherited.retain(|inherited_index| {
            !members.iter().any(|member_index| {
                self.candidates[*member_index]
                    .declaration
                    .as_ref()
                    .is_some_and(|declaration| declaration.modifiers.is_override)
                    && same_callable_signature(
                        self.candidates[*member_index].declaration.as_ref(),
                        self.candidates[*inherited_index].declaration.as_ref(),
                    )
            })
        });
        unique_candidates(members.into_iter().chain(inherited).collect())
    }
}
