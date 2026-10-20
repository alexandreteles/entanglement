use super::*;

mod types;

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
                owners.contains(&(candidate.package.to_vec(), candidate.owner.to_vec()))
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
        let objects = classes.iter().copied().filter(|class| {
            class
                .declaration
                .is_some_and(|item| item.kind == DeclarationKind::Object)
        });
        let companions = self.candidates.iter().filter(|candidate| {
            candidate
                .declaration
                .is_some_and(|item| item.kind == DeclarationKind::CompanionObject)
                && owners.contains(&(candidate.package.to_vec(), candidate.owner.to_vec()))
        });
        objects
            .chain(companions)
            .flat_map(|class| {
                let owner = extended_owner(class.owner, &class.name);
                self.named(name)
                    .filter(move |candidate| {
                        candidate.package == class.package && candidate.owner == owner
                    })
                    .map(|candidate| candidate.candidate_index)
            })
            .collect()
    }
    pub(super) fn is_static_type_qualifier(&self, file: usize, reference: &Reference) -> bool {
        if reference.path.len() < 2 {
            return false;
        }
        let receiver = reference.path[0].as_str();
        if receiver == "this" || receiver.starts_with("this@") || receiver == "super" {
            return false;
        }
        if self
            .graph
            .binding_type(file, receiver, reference.context_id, reference.start_byte)
            .is_some()
            || self.graph.has_local_binding(
                file,
                receiver,
                reference.context_id,
                reference.start_byte,
            )
        {
            return false;
        }
        if self.imported_type(file, reference, receiver).is_some() {
            return true;
        }
        let package = self.semantic_package(file, reference.context_id);
        let candidates = self.package_types(receiver, package).collect::<Vec<_>>();
        candidates.len() == 1
    }
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
            if !seen.insert((*class.symbol).clone()) {
                continue;
            }
            for parent in self.direct_supertypes(class) {
                inherited.extend(self.members_of_type_seen(
                    file,
                    reference,
                    name,
                    &candidate_type_name(parent),
                    false,
                    seen,
                ));
            }
        }
        inherited
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
                    .is_some_and(|declaration| declaration.modifiers.is_override)
                    && same_callable_signature(
                        self.candidates[*member_index].declaration,
                        self.candidates[*inherited_index].declaration,
                    )
            })
        });
        unique_candidates(members.into_iter().chain(inherited).collect())
    }
    pub(super) fn extensions_for_type(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
        type_name: &str,
    ) -> Vec<usize> {
        let current_package = self.semantic_package(file, reference.context_id);
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
                                && &import.path[..import.path.len().saturating_sub(1)]
                                    == candidate.package
                        }
                    })
            })
            .filter(|candidate| {
                let Some(receiver) = candidate
                    .signature()
                    .and_then(|signature| signature.receiver_type.as_deref())
                else {
                    return false;
                };
                let actual = self.canonical_type_names(
                    candidate.file,
                    candidate.context_id,
                    candidate.package,
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
