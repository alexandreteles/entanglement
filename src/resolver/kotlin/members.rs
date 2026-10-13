use super::*;

impl Index<'_> {
    pub(super) fn members_of_type(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
        type_name: &str,
    ) -> Vec<usize> {
        self.members_of_type_seen(
            file,
            reference,
            name,
            type_name,
            self.is_static_type_qualifier(file, reference),
            &mut BTreeSet::new(),
        )
    }

    pub(super) fn members_of_type_seen(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
        type_name: &str,
        static_qualifier: bool,
        seen: &mut BTreeSet<SymbolId>,
    ) -> Vec<usize> {
        let classes = self.type_candidates(file, reference, type_name);
        if let [class] = classes.as_slice()
            && let Some(alias) = class
                .declaration
                .as_ref()
                .and_then(|declaration| declaration.alias_target.as_deref())
            && alias.rsplit('.').next().unwrap_or(alias)
                != type_name.rsplit('.').next().unwrap_or(type_name)
        {
            let members =
                self.members_of_type_seen(file, reference, name, alias, static_qualifier, seen);
            if !members.is_empty() {
                return members;
            }
        }
        let owners = owner_paths(&classes);
        let mut members = self.direct_member_candidates(file, name, &owners, static_qualifier);
        if static_qualifier {
            members.extend(self.static_member_candidates(name, &owners, &classes));
            return unique_candidates(members);
        }
        let inherited = self.inherited_member_candidates(file, reference, name, &classes, seen);
        self.merge_inherited_members(members, inherited)
    }

    pub(super) fn direct_member_candidates(
        &self,
        file: usize,
        name: &str,
        owners: &BTreeSet<(Vec<String>, Vec<String>)>,
        static_qualifier: bool,
    ) -> Vec<usize> {
        if static_qualifier {
            return Vec::new();
        }
        self.named(name)
            .filter(|candidate| !candidate.owner.is_empty())
            .filter(|candidate| {
                owners.contains(&(candidate.package.clone(), candidate.owner.clone()))
            })
            .filter(|candidate| self.visible_in_source_set(file, candidate))
            .map(|candidate| candidate.candidate_index)
            .collect()
    }

    pub(super) fn static_member_candidates(
        &self,
        name: &str,
        owners: &BTreeSet<(Vec<String>, Vec<String>)>,
        classes: &[&Candidate],
    ) -> Vec<usize> {
        let mut members = self.object_member_candidates(name, classes);
        for companion in self.candidates.iter().filter(|candidate| {
            candidate
                .declaration
                .as_ref()
                .is_some_and(|item| item.kind == crate::graph::DeclarationKind::CompanionObject)
                && owners.contains(&(candidate.package.clone(), candidate.owner.clone()))
        }) {
            let companion_owner = extended_owner(&companion.owner, &companion.name);
            members.extend(self.named(name).filter_map(|candidate| {
                (candidate.package == companion.package && candidate.owner == companion_owner)
                    .then_some(candidate.candidate_index)
            }));
        }
        members
    }

    pub(super) fn object_member_candidates(
        &self,
        name: &str,
        classes: &[&Candidate],
    ) -> Vec<usize> {
        classes
            .iter()
            .filter(|class| {
                class.declaration.as_ref().is_some_and(|declaration| {
                    declaration.kind == crate::graph::DeclarationKind::Object
                })
            })
            .flat_map(|class| {
                let owner = extended_owner(&class.owner, &class.name);
                self.named(name)
                    .filter(move |candidate| {
                        candidate.package == class.package && candidate.owner == owner
                    })
                    .map(|candidate| candidate.candidate_index)
            })
            .collect()
    }
}
