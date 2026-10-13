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
        let mut members = self.object_member_candidates(name, classes);
        for companion in self.candidates.iter().filter(|candidate| {
            candidate
                .declaration
                .is_some_and(|item| item.kind == DeclarationKind::CompanionObject)
                && owners.contains(&(candidate.package.to_vec(), candidate.owner.to_vec()))
        }) {
            let companion_owner = extended_owner(companion.owner, companion.name);
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
                class
                    .declaration
                    .is_some_and(|declaration| declaration.kind == DeclarationKind::Object)
            })
            .flat_map(|class| {
                let owner = extended_owner(class.owner, class.name);
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
        let candidates = self
            .named(receiver)
            .filter(|candidate| {
                candidate.package == package
                    && candidate.owner.is_empty()
                    && is_type_declaration(candidate)
            })
            .collect::<Vec<_>>();
        candidates.len() == 1
    }
    pub(super) fn type_candidates<'b>(
        &'b self,
        file: usize,
        reference: &Reference,
        type_name: &str,
    ) -> Vec<&'b Candidate<'b>> {
        let normalized = normalize_type(type_name);
        let short_name = normalized.rsplit('.').next().unwrap_or(&normalized);
        let package = self.semantic_package(file, reference.context_id);
        let imported = self.imported_candidates(file, reference, short_name, false);
        let mut classes = self
            .named(short_name)
            .filter(|candidate| is_type_declaration(candidate))
            .filter(|candidate| self.visible_in_source_set(file, candidate))
            .filter(|candidate| {
                if normalized.contains('.') {
                    candidate_type_name(candidate) == normalized
                } else if !imported.is_empty() {
                    imported.contains(&candidate.candidate_index)
                } else {
                    candidate.package == package && candidate.owner.is_empty()
                }
            })
            .collect::<Vec<_>>();
        classes.dedup_by_key(|candidate| candidate.symbol);
        classes
    }
    pub(super) fn resolve_declared_type(
        &self,
        file: usize,
        context_id: usize,
        type_name: &str,
        package: &[String],
    ) -> Vec<&Candidate<'_>> {
        let name = type_name.rsplit('.').next().unwrap_or(type_name);
        if let Some((qualified_package, _)) = type_name.rsplit_once('.') {
            let qualified = qualified_package
                .split('.')
                .map(str::to_owned)
                .chain(std::iter::once(name.to_owned()))
                .collect::<Vec<_>>();
            return self
                .named(name)
                .filter(|candidate| {
                    candidate_type_segments(candidate) == qualified
                        && is_type_declaration(candidate)
                })
                .collect();
        }
        let imported = self
            .imports(file, context_id)
            .filter(|import| !import.namespace && imported_name(import) == Some(name))
            .flat_map(|import| {
                let imported_name = import.path.last().map(String::as_str).unwrap_or(name);
                self.named(imported_name).filter(move |candidate| {
                    candidate_type_segments(candidate) == import.path
                        && is_type_declaration(candidate)
                })
            })
            .collect::<Vec<_>>();
        if !imported.is_empty() {
            return imported;
        }
        self.named(name)
            .filter(|candidate| {
                candidate.package == package
                    && candidate.owner.is_empty()
                    && is_type_declaration(candidate)
            })
            .collect()
    }
    pub(super) fn canonical_type_names(
        &self,
        file: usize,
        context_id: usize,
        package: &[String],
        type_name: &str,
        seen: &mut BTreeSet<String>,
    ) -> BTreeSet<String> {
        let normalized = normalize_type(type_name);
        if !seen.insert(normalized.clone()) {
            return BTreeSet::new();
        }
        let declarations = self.resolve_declared_type(file, context_id, &normalized, package);
        const BUILTINS: &[&str] = &[
            "Any", "Nothing", "Unit", "Boolean", "Byte", "Short", "Int", "Long", "Float", "Double",
            "Char", "String",
        ];
        match declarations.as_slice() {
            [candidate] => match candidate
                .declaration
                .and_then(|declaration| declaration.alias_target.as_deref())
            {
                Some(alias) => self.canonical_type_names(
                    candidate.file,
                    candidate.context_id,
                    candidate.package,
                    alias,
                    seen,
                ),
                None => [candidate_type_name(candidate)].into_iter().collect(),
            },
            [] if BUILTINS.contains(&normalized.as_str()) || normalized.contains('.') => {
                [normalized].into_iter().collect()
            }
            _ => BTreeSet::new(),
        }
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
            let Some(declaration) = class.declaration else {
                continue;
            };
            for supertype in &declaration.supertypes {
                let parents = self.resolve_declared_type(
                    class.file,
                    class.context_id,
                    supertype,
                    class.package,
                );
                for parent in parents {
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
