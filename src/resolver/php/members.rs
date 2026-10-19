//! Select PHP members with visibility, inheritance, and trait rules.

use super::*;

#[derive(Clone)]
pub(super) struct Member {
    pub candidate: usize,
    pub class: Option<usize>,
    pub visibility: Option<SemanticVisibility>,
    pub exact_receiver: bool,
    pub is_final: bool,
}

impl Member {
    pub fn new(candidate: usize) -> Self {
        Self {
            candidate,
            class: None,
            visibility: None,
            exact_receiver: false,
            is_final: false,
        }
    }
}

impl Index<'_> {
    pub(super) fn member_reference(
        &self,
        file: usize,
        reference: &Reference,
        php: &PhpReference,
        seen: &mut HashSet<usize>,
    ) -> Vec<Member> {
        let Some(receiver) = &php.receiver else {
            return Vec::new();
        };
        let classes = self.receiver_classes(file, reference, php, receiver, seen);
        let kind = match php.kind {
            PhpReferenceKind::Property | PhpReferenceKind::StaticProperty => {
                PhpSymbolKind::Property
            }
            PhpReferenceKind::ClassConstant => PhpSymbolKind::Constant,
            _ => PhpSymbolKind::Method,
        };
        let exact = receiver.starts_with("new ")
            || receiver.starts_with("@new:")
            || self
                .receiver_binding(file, reference, receiver)
                .is_some_and(|binding| {
                    binding.immutable
                        && binding
                            .alias_target
                            .as_deref()
                            .is_some_and(|alias| alias.starts_with("@new:"))
                });
        let mut results = Vec::new();
        for class in classes {
            if self.candidates[class].php.conditional {
                return Vec::new();
            }
            let mut members = self.type_members(class, kind, &php.name, &mut HashSet::new());
            members.retain(|member| {
                self.legal_member(file, reference, php, member)
                    && self.accessible(file, reference, php, member)
            });
            if members.is_empty() {
                return Vec::new();
            }
            for member in &mut members {
                member.exact_receiver = exact
                    || self.candidates[class]
                        .declaration
                        .is_some_and(|declaration| declaration.modifiers.is_final);
            }
            results.extend(members);
        }
        results
    }

    fn legal_member(
        &self,
        file: usize,
        reference: &Reference,
        php: &PhpReference,
        member: &Member,
    ) -> bool {
        let candidate = &self.candidates[member.candidate];
        match php.kind {
            PhpReferenceKind::StaticProperty => candidate.php.is_static,
            PhpReferenceKind::Property => !candidate.php.is_static,
            PhpReferenceKind::StaticMethod if !candidate.php.is_static => {
                php.receiver.as_deref().is_some_and(|receiver| {
                    matches!(
                        receiver.to_ascii_lowercase().as_str(),
                        "self" | "parent" | "static"
                    )
                }) && self.candidates.iter().any(|caller| {
                    caller.id.file_index == file
                        && caller.context == reference.context_id
                        && Some(caller.php.start_byte) == reference.execution_owner
                        && !caller.php.is_static
                        && caller.php.kind == PhpSymbolKind::Method
                })
            }
            _ => true,
        }
    }

    fn receiver_classes(
        &self,
        file: usize,
        reference: &Reference,
        php: &PhpReference,
        receiver: &str,
        seen: &mut HashSet<usize>,
    ) -> Vec<usize> {
        if !seen.insert(php.start_byte) {
            return Vec::new();
        }
        if let Some(start) = php.receiver_reference {
            return self.typed_receiver(file, reference, start, seen);
        }
        if matches!(
            receiver.to_ascii_lowercase().as_str(),
            "$this" | "self" | "parent" | "static"
        ) {
            let Some(name) = php.class.as_deref() else {
                return Vec::new();
            };
            let classes = self.classes(
                file,
                reference.context_id,
                reference.start_byte,
                &format!("\\{name}"),
            );
            let classes = classes
                .into_iter()
                .flat_map(|class| self.consumers(class, &mut HashSet::new()))
                .collect::<Vec<_>>();
            return if receiver.eq_ignore_ascii_case("parent") {
                classes
                    .into_iter()
                    .flat_map(|class| self.parents(class))
                    .filter(|parent| !self.candidates[*parent].php.is_interface)
                    .collect()
            } else {
                classes
            };
        }
        let name = if let Some(name) = receiver
            .strip_prefix("new ")
            .or_else(|| receiver.strip_prefix("@new:"))
        {
            name.to_owned()
        } else if receiver.starts_with('$') {
            let Some(binding) = self
                .receiver_binding(file, reference, receiver)
                .filter(|binding| binding.immutable)
            else {
                return Vec::new();
            };
            let Some(name) = binding.type_name.as_deref() else {
                return Vec::new();
            };
            name.trim_start_matches('?').to_owned()
        } else {
            receiver.to_owned()
        };
        self.classes(file, reference.context_id, reference.start_byte, &name)
    }

    fn typed_receiver(
        &self,
        file: usize,
        reference: &Reference,
        start: usize,
        seen: &mut HashSet<usize>,
    ) -> Vec<usize> {
        let Some(inner) =
            self.graph.file(file).references().find(|inner| {
                inner.start_byte == start && inner.context_id == reference.context_id
            })
        else {
            return Vec::new();
        };
        let Some(php) = self
            .facts(file, reference.context_id)
            .and_then(|facts| facts.references.iter().find(|php| php.start_byte == start))
        else {
            return Vec::new();
        };
        let candidates = match php.kind {
            PhpReferenceKind::Function if php.callable => {
                self.lookup(file, reference.context_id, php)
            }
            PhpReferenceKind::Method | PhpReferenceKind::StaticMethod if php.callable => self
                .member_reference(file, inner, php, seen)
                .into_iter()
                .map(|member| member.candidate)
                .collect(),
            PhpReferenceKind::Property | PhpReferenceKind::StaticProperty => self
                .member_reference(file, inner, php, seen)
                .into_iter()
                .map(|member| member.candidate)
                .collect(),
            _ => return Vec::new(),
        };
        let [candidate] = candidates.as_slice() else {
            return Vec::new();
        };
        let candidate = &self.candidates[*candidate];
        if candidate.php.conditional {
            return Vec::new();
        }
        let Some(declaration) = candidate.declaration else {
            return Vec::new();
        };
        let type_name = if candidate.php.kind == PhpSymbolKind::Property {
            declaration.declared_type.as_deref()
        } else {
            declaration
                .signature
                .as_ref()
                .and_then(|signature| signature.return_type.as_deref())
        };
        let Some(type_name) = type_name.map(|name| name.trim_start_matches('?')) else {
            return Vec::new();
        };
        let namespace_start = self
            .facts(candidate.id.file_index, candidate.context)
            .into_iter()
            .flat_map(|facts| &facts.namespaces)
            .filter(|namespace| {
                namespace.range.start_byte <= candidate.php.start_byte
                    && candidate.php.start_byte < namespace.range.end_byte
            })
            .min_by_key(|namespace| namespace.range.end_byte - namespace.range.start_byte)
            .map_or(0, |namespace| namespace.range.start_byte);
        self.lookup(
            candidate.id.file_index,
            candidate.context,
            &PhpReference {
                start_byte: candidate.php.start_byte,
                name: type_name.to_owned(),
                kind: PhpReferenceKind::Class,
                receiver: None,
                receiver_reference: None,
                namespace_start,
                class: candidate.owner.clone(),
                callable: false,
            },
        )
    }

    pub(super) fn receiver_binding(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
    ) -> Option<&crate::graph::BindingType> {
        let semantic = self.graph.semantic(
            file,
            reference.context_id,
            Some(ResolutionFamily::PhpNamespaces),
        )?;
        let mut bindings = semantic
            .bindings
            .iter()
            .filter(|binding| {
                binding.name == name
                    && binding.scope_start <= reference.start_byte
                    && reference.start_byte < binding.scope_end
                    && self.callable_owner(file, binding.name_start_byte)
                        == reference.execution_owner
            })
            .collect::<Vec<_>>();
        bindings.sort_by_key(|binding| std::cmp::Reverse(binding.scope_start));
        let binding = *bindings.first()?;
        bindings
            .iter()
            .all(|other| {
                other.type_name == binding.type_name && other.alias_target == binding.alias_target
            })
            .then_some(binding)
    }

    fn callable_owner(&self, file: usize, position: usize) -> Option<usize> {
        self.graph
            .file(file)
            .analysis
            .functions
            .iter()
            .filter(|function| function.start_byte <= position && position < function.end_byte)
            .min_by_key(|function| function.end_byte - function.start_byte)
            .map(|function| function.start_byte)
    }

    pub(super) fn parents(&self, class: usize) -> Vec<usize> {
        let candidate = &self.candidates[class];
        candidate
            .declaration
            .into_iter()
            .flat_map(|declaration| &declaration.supertypes)
            .flat_map(|parent| {
                self.classes(
                    candidate.id.file_index,
                    candidate.context,
                    candidate.php.start_byte,
                    parent,
                )
            })
            .collect()
    }

    pub(super) fn consumers(&self, class: usize, visited: &mut HashSet<usize>) -> Vec<usize> {
        if !self.candidates[class].php.is_trait {
            return vec![class];
        }
        if !visited.insert(class) {
            return Vec::new();
        }
        let mut consumers = Vec::new();
        for (index, _) in self
            .candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| candidate.php.kind == PhpSymbolKind::Class)
        {
            if self
                .traits(index)
                .iter()
                .any(|trait_index| *trait_index == class)
            {
                consumers.extend(self.consumers(index, visited));
            }
        }
        consumers
    }

    fn traits(&self, class: usize) -> Vec<usize> {
        let candidate = &self.candidates[class];
        self.facts(candidate.id.file_index, candidate.context)
            .into_iter()
            .flat_map(|facts| &facts.traits)
            .filter(|usage| usage.owner.eq_ignore_ascii_case(&candidate.name))
            .flat_map(|usage| {
                usage.traits.iter().flat_map(|name| {
                    self.classes(
                        candidate.id.file_index,
                        candidate.context,
                        usage.start_byte,
                        name,
                    )
                })
            })
            .collect()
    }

    pub(super) fn type_members(
        &self,
        class: usize,
        kind: PhpSymbolKind,
        name: &str,
        visited: &mut HashSet<usize>,
    ) -> Vec<Member> {
        if !visited.insert(class) {
            return Vec::new();
        }
        let candidate = &self.candidates[class];
        let direct = self
            .members
            .get(&(
                candidate.name.to_ascii_lowercase(),
                kind,
                names::key(kind, name),
            ))
            .into_iter()
            .flatten()
            .filter(|index| {
                self.candidates[**index].id.file_index == candidate.id.file_index
                    && self.candidates[**index].context == candidate.context
            })
            .map(|index| Member {
                class: Some(class),
                ..Member::new(*index)
            })
            .collect::<Vec<_>>();
        if !direct.is_empty() {
            return direct;
        }
        let mut traits = self.trait_members(class, kind, name, visited);
        if !traits.is_empty() {
            for member in &mut traits {
                member.class = Some(class);
            }
            return traits;
        }
        let parents = self.parents(class);
        let concrete = parents
            .iter()
            .filter(|parent| !self.candidates[**parent].php.is_interface)
            .copied()
            .collect::<Vec<_>>();
        let parents = if concrete.is_empty() {
            parents
        } else {
            concrete
        };
        parents
            .into_iter()
            .flat_map(|parent| self.type_members(parent, kind, name, &mut visited.clone()))
            .collect()
    }

    fn trait_members(
        &self,
        class: usize,
        kind: PhpSymbolKind,
        name: &str,
        visited: &HashSet<usize>,
    ) -> Vec<Member> {
        let candidate = &self.candidates[class];
        self.facts(candidate.id.file_index, candidate.context)
            .into_iter()
            .flat_map(|facts| &facts.traits)
            .filter(|usage| usage.owner.eq_ignore_ascii_case(&candidate.name))
            .flat_map(|usage| {
                usage
                    .traits
                    .iter()
                    .flat_map(|trait_name| {
                        self.classes(
                            candidate.id.file_index,
                            candidate.context,
                            usage.start_byte,
                            trait_name,
                        )
                    })
                    .flat_map(move |trait_index| {
                        self.trait_contributions(candidate, usage, trait_index, kind, name, visited)
                    })
            })
            .collect()
    }

    fn trait_contributions(
        &self,
        candidate: &Candidate<'_>,
        usage: &crate::graph::PhpTraitUse,
        trait_index: usize,
        kind: PhpSymbolKind,
        name: &str,
        visited: &HashSet<usize>,
    ) -> Vec<Member> {
        let excluded = usage.adaptations.iter().any(|adaptation| {
            adaptation.method.eq_ignore_ascii_case(name)
                && adaptation.instead_of.iter().any(|excluded| {
                    self.classes(
                        candidate.id.file_index,
                        candidate.context,
                        usage.start_byte,
                        excluded,
                    )
                    .contains(&trait_index)
                })
        });
        let adaptation = usage
            .adaptations
            .iter()
            .filter(|adaptation| {
                adaptation.alias.is_none()
                    && adaptation.instead_of.is_empty()
                    && adaptation.method.eq_ignore_ascii_case(name)
                    && self.adapts_trait(
                        candidate,
                        usage.start_byte,
                        adaptation.trait_name.as_deref(),
                        trait_index,
                    )
            })
            .last();
        let direct = (!excluded).then_some((name, adaptation));
        let aliases = usage
            .adaptations
            .iter()
            .filter(|adaptation| {
                adaptation
                    .alias
                    .as_deref()
                    .is_some_and(|alias| alias.eq_ignore_ascii_case(name))
                    && self.adapts_trait(
                        candidate,
                        usage.start_byte,
                        adaptation.trait_name.as_deref(),
                        trait_index,
                    )
            })
            .map(|adaptation| (adaptation.method.as_str(), Some(adaptation)));
        direct
            .into_iter()
            .chain(aliases)
            .flat_map(|(source_name, adaptation)| {
                self.type_members(trait_index, kind, source_name, &mut visited.clone())
                    .into_iter()
                    .map(move |mut member| {
                        if let Some(adaptation) = adaptation {
                            member.visibility = adaptation.visibility.or(member.visibility);
                            member.is_final |= adaptation.is_final;
                        }
                        member
                    })
            })
            .collect()
    }

    fn adapts_trait(
        &self,
        candidate: &Candidate<'_>,
        position: usize,
        name: Option<&str>,
        trait_index: usize,
    ) -> bool {
        name.is_none_or(|name| {
            self.classes(candidate.id.file_index, candidate.context, position, name)
                .contains(&trait_index)
        })
    }

    pub(super) fn accessible(
        &self,
        file: usize,
        reference: &Reference,
        php: &PhpReference,
        member: &Member,
    ) -> bool {
        let candidate = &self.candidates[member.candidate];
        let read_visibility = member
            .visibility
            .or(candidate
                .declaration
                .map(|declaration| declaration.visibility))
            .unwrap_or(SemanticVisibility::Unknown);
        let visibility = if candidate.php.kind == PhpSymbolKind::Property
            && matches!(
                reference.access,
                Some(
                    crate::model::ReferenceAccess::Write | crate::model::ReferenceAccess::ReadWrite
                )
            ) {
            candidate.php.setter_visibility.unwrap_or(read_visibility)
        } else {
            read_visibility
        };
        if visibility == SemanticVisibility::Public {
            return true;
        }
        let Some(lexical) = &php.class else {
            return false;
        };
        let owner = member
            .class
            .map(|class| self.candidates[class].name.as_str())
            .or(candidate.owner.as_deref());
        let Some(owner) = owner else {
            return false;
        };
        if lexical.eq_ignore_ascii_case(owner) {
            return true;
        }
        if self
            .classes(
                file,
                reference.context_id,
                reference.start_byte,
                &format!("\\{lexical}"),
            )
            .into_iter()
            .filter(|class| self.candidates[*class].php.is_trait)
            .flat_map(|class| self.consumers(class, &mut HashSet::new()))
            .any(|class| self.candidates[class].name.eq_ignore_ascii_case(owner))
        {
            return true;
        }
        if visibility != SemanticVisibility::Protected {
            return false;
        }
        self.classes(
            file,
            reference.context_id,
            reference.start_byte,
            &format!("\\{lexical}"),
        )
        .into_iter()
        .any(|class| self.extends(class, owner, &mut HashSet::new()))
    }

    fn extends(&self, class: usize, owner: &str, visited: &mut HashSet<usize>) -> bool {
        if self.candidates[class].name.eq_ignore_ascii_case(owner) {
            return true;
        }
        visited.insert(class)
            && self
                .parents(class)
                .into_iter()
                .any(|parent| self.extends(parent, owner, visited))
    }

    pub(super) fn dispatch(&self, php: &PhpReference, member: &Member) -> CallDispatch {
        let candidate = &self.candidates[member.candidate];
        if candidate.php.kind == PhpSymbolKind::Function {
            return CallDispatch::Direct;
        }
        if candidate.php.kind == PhpSymbolKind::Class {
            return CallDispatch::Direct;
        }
        let declaration = candidate.declaration;
        if declaration.is_some_and(|declaration| {
            declaration.modifiers.is_abstract || declaration.is_interface_member
        }) {
            return CallDispatch::Virtual;
        }
        let receiver = php.receiver.as_deref().unwrap_or_default();
        let direct = member.visibility == Some(SemanticVisibility::Private)
            || member.is_final
            || member.exact_receiver
            || member.class.is_some_and(|class| {
                self.candidates[class]
                    .declaration
                    .is_some_and(|declaration| declaration.modifiers.is_final)
            })
            || declaration.is_some_and(|declaration| {
                declaration.modifiers.is_final
                    || declaration.visibility == SemanticVisibility::Private
            })
            || (php.kind == PhpReferenceKind::StaticMethod
                && !receiver.eq_ignore_ascii_case("static"));
        if direct {
            CallDispatch::Direct
        } else {
            CallDispatch::Virtual
        }
    }
}
