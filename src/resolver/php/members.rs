//! Select PHP members with visibility, inheritance, and trait rules.

use super::*;
use crate::model::ReferenceAccess::{ReadWrite, Write};
use PhpReferenceKind as RefKind;
use PhpSymbolKind as SymbolKind;
use SemanticVisibility::{Private, Protected, Public};

#[derive(Clone, Default)]
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
            ..Self::default()
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
        let Some(receiver) = php.receiver.as_deref() else {
            return Vec::new();
        };
        let Some(classes) = self.receiver_classes(file, reference, php, receiver, seen) else {
            return Vec::new();
        };
        let exact = receiver.starts_with("new ")
            || self
                .receiver_binding(file, reference, receiver)
                .is_some_and(|binding| {
                    binding.immutable && binding.alias_target.as_deref() == Some("@new")
                });
        let mut results = Vec::new();
        for class in classes {
            if self.candidates[class].php.conditional {
                return Vec::new();
            }
            let mut members = self.type_members(
                class,
                names::reference_kind(php.kind),
                &php.name,
                &mut HashSet::new(),
            );
            members.retain(|member| {
                self.legal_member(file, reference, php, member)
                    && self.accessible(file, reference, php, member)
            });
            if members.is_empty() {
                return Vec::new();
            }
            for member in &mut members {
                member.exact_receiver =
                    exact || self.candidates[class].declaration.modifiers.is_final;
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
            RefKind::StaticProperty => candidate.php.is_static,
            RefKind::Property => !candidate.php.is_static,
            RefKind::StaticMethod if !candidate.php.is_static => {
                php.receiver.as_deref().is_some_and(|receiver| {
                    ["self", "parent", "static"]
                        .iter()
                        .any(|name| receiver.eq_ignore_ascii_case(name))
                }) && self.candidates.iter().any(|caller| {
                    caller.site.file == file
                        && caller.site.context == reference.context_id
                        && Some(caller.site.position) == reference.execution_owner
                        && !caller.php.is_static
                        && caller.php.kind == SymbolKind::Method
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
    ) -> Option<Vec<usize>> {
        if !seen.insert(php.start_byte) {
            return None;
        }
        let site = Site::at(file, reference);
        match php.receiver_reference {
            Some(start) => Some(self.typed_receiver(file, reference, start, seen)),
            None if ["$this", "self", "parent", "static"]
                .iter()
                .any(|name| receiver.eq_ignore_ascii_case(name)) =>
            {
                Some(self.lookup(
                    site,
                    match receiver.eq_ignore_ascii_case("$this") {
                        true => "self",
                        false => receiver,
                    },
                    SymbolKind::Class,
                    php.class.as_deref(),
                ))
            }
            None => {
                let name = match receiver.strip_prefix("new ") {
                    Some(name) => name,
                    None if receiver.starts_with('$') => self
                        .receiver_binding(file, reference, receiver)
                        .filter(|binding| binding.immutable)?
                        .type_name
                        .as_deref()?
                        .trim_start_matches('?'),
                    None => receiver,
                };
                Some(self.lookup(site, name, SymbolKind::Class, None))
            }
        }
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
        let candidates = match (php.kind, php.callable) {
            (RefKind::Function, true) => self.lookup(
                Site::at(file, inner),
                &php.name,
                SymbolKind::Function,
                php.class.as_deref(),
            ),
            (RefKind::Method | RefKind::StaticMethod, true)
            | (RefKind::Property | RefKind::StaticProperty, _) => self
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
        let type_name = match candidate.php.kind {
            SymbolKind::Property => candidate.declaration.declared_type.as_deref(),
            _ => candidate
                .declaration
                .signature
                .as_ref()
                .and_then(|signature| signature.return_type.as_deref()),
        };
        type_name.map_or_else(Vec::new, |name| {
            self.lookup(
                candidate.site,
                name.trim_start_matches('?'),
                SymbolKind::Class,
                candidate.owner,
            )
        })
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
        let mut bindings = semantic.bindings.iter().filter(|binding| {
            binding.name == name
                && binding.scope_start <= reference.start_byte
                && reference.start_byte < binding.scope_end
                && self.callable_owner(file, binding.name_start_byte) == reference.execution_owner
        });
        let binding = bindings
            .clone()
            .min_by_key(|binding| std::cmp::Reverse(binding.scope_start))?;
        bindings
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
            .supertypes
            .iter()
            .flat_map(|parent| self.lookup(candidate.site, parent, SymbolKind::Class, None))
            .collect()
    }

    pub(super) fn consumers(&self, class: usize, visited: &mut HashSet<usize>) -> Vec<usize> {
        match (self.candidates[class].php.is_trait, visited.insert(class)) {
            (false, _) => vec![class],
            (true, false) => Vec::new(),
            (true, true) => self
                .candidates
                .iter()
                .enumerate()
                .filter(|(index, candidate)| {
                    candidate.php.kind == SymbolKind::Class && self.traits(*index).contains(&class)
                })
                .flat_map(|(index, _)| self.consumers(index, visited))
                .collect(),
        }
    }

    fn trait_uses(&self, class: usize) -> impl Iterator<Item = &crate::graph::PhpTraitUse> {
        let candidate = &self.candidates[class];
        self.facts(candidate.site.file, candidate.site.context)
            .into_iter()
            .flat_map(|facts| &facts.traits)
            .filter(|usage| usage.owner.eq_ignore_ascii_case(&candidate.name))
    }

    fn traits(&self, class: usize) -> Vec<usize> {
        self.trait_uses(class)
            .flat_map(|usage| {
                usage.traits.iter().flat_map(|name| {
                    self.lookup(self.candidates[class].site, name, SymbolKind::Class, None)
                })
            })
            .collect()
    }

    pub(super) fn type_members(
        &self,
        class: usize,
        kind: SymbolKind,
        name: &str,
        visited: &mut HashSet<usize>,
    ) -> Vec<Member> {
        if !visited.insert(class) {
            return Vec::new();
        }
        let candidate = &self.candidates[class];
        let direct = self
            .symbols
            .get(&(
                Some(candidate.name.to_ascii_lowercase()),
                kind,
                names::key(kind, name),
            ))
            .into_iter()
            .flatten()
            .filter(|index| {
                self.candidates[**index].site.file == candidate.site.file
                    && self.candidates[**index].site.context == candidate.site.context
            })
            .map(|index| Member {
                class: Some(class),
                ..Member::new(*index)
            })
            .collect::<Vec<_>>();
        if !direct.is_empty() {
            return direct;
        }
        let visited = &*visited;
        let traits = self
            .trait_uses(class)
            .flat_map(|usage| {
                usage
                    .traits
                    .iter()
                    .flat_map(|trait_name| {
                        self.lookup(candidate.site, trait_name, SymbolKind::Class, None)
                    })
                    .flat_map(move |trait_index| {
                        self.trait_contributions(
                            candidate.site,
                            usage,
                            trait_index,
                            kind,
                            name,
                            visited,
                        )
                    })
            })
            .map(|member| Member {
                class: Some(class),
                ..member
            })
            .collect::<Vec<_>>();
        if !traits.is_empty() {
            return traits;
        }
        let parents = self.parents(class);
        let concrete = parents
            .iter()
            .any(|parent| !self.candidates[*parent].php.is_interface);
        parents
            .into_iter()
            .filter(|parent| !concrete || !self.candidates[*parent].php.is_interface)
            .flat_map(|parent| self.type_members(parent, kind, name, &mut visited.clone()))
            .collect()
    }

    fn trait_contributions(
        &self,
        site: Site,
        usage: &crate::graph::PhpTraitUse,
        trait_index: usize,
        kind: SymbolKind,
        name: &str,
        visited: &HashSet<usize>,
    ) -> Vec<Member> {
        let excluded = usage.adaptations.iter().any(|adaptation| {
            adaptation.method.eq_ignore_ascii_case(name)
                && adaptation.instead_of.iter().any(|excluded| {
                    self.lookup(site, excluded, SymbolKind::Class, None)
                        .contains(&trait_index)
                })
        });
        let applies = |adaptation: &&crate::graph::PhpTraitAdaptation| {
            adaptation.trait_name.as_deref().is_none_or(|name| {
                self.lookup(site, name, SymbolKind::Class, None)
                    .contains(&trait_index)
            })
        };
        let adaptation = usage
            .adaptations
            .iter()
            .rev()
            .filter(applies)
            .find(|adaptation| {
                adaptation.alias.is_none()
                    && adaptation.instead_of.is_empty()
                    && adaptation.method.eq_ignore_ascii_case(name)
            });
        let aliases = usage
            .adaptations
            .iter()
            .filter(applies)
            .filter(|adaptation| {
                adaptation
                    .alias
                    .as_deref()
                    .is_some_and(|alias| alias.eq_ignore_ascii_case(name))
            })
            .map(|adaptation| (adaptation.method.as_str(), Some(adaptation)));
        (!excluded)
            .then_some((name, adaptation))
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
            .unwrap_or(candidate.declaration.visibility);
        let visibility = match (candidate.php.kind, reference.access) {
            (SymbolKind::Property, Some(Write | ReadWrite)) => {
                candidate.php.setter_visibility.unwrap_or(read_visibility)
            }
            _ => read_visibility,
        };
        if visibility == Public {
            return true;
        }
        let Some((lexical, owner)) = php.class.as_deref().zip(
            member
                .class
                .map(|class| self.candidates[class].name.as_str())
                .or(candidate.owner),
        ) else {
            return false;
        };
        if lexical.eq_ignore_ascii_case(owner) {
            return true;
        }
        let classes = self.lookup(
            Site::at(file, reference),
            &format!("\\{lexical}"),
            SymbolKind::Class,
            None,
        );
        classes
            .iter()
            .filter(|class| self.candidates[**class].php.is_trait)
            .flat_map(|class| self.consumers(*class, &mut HashSet::new()))
            .any(|class| self.candidates[class].name.eq_ignore_ascii_case(owner))
            || (visibility == Protected
                && classes
                    .into_iter()
                    .any(|class| self.extends(class, owner, &mut HashSet::new())))
    }

    fn extends(&self, class: usize, owner: &str, visited: &mut HashSet<usize>) -> bool {
        self.candidates[class].name.eq_ignore_ascii_case(owner)
            || (visited.insert(class)
                && self
                    .parents(class)
                    .into_iter()
                    .any(|parent| self.extends(parent, owner, visited)))
    }

    pub(super) fn dispatch(&self, php: &PhpReference, member: &Member) -> CallDispatch {
        let candidate = &self.candidates[member.candidate];
        let declaration = candidate.declaration;
        match candidate.php.kind {
            SymbolKind::Function | SymbolKind::Class => CallDispatch::Direct,
            _ if declaration.modifiers.is_abstract || declaration.is_interface_member => {
                CallDispatch::Virtual
            }
            _ if member.visibility == Some(Private)
                || member.is_final
                || member.exact_receiver
                || declaration.modifiers.is_final
                || declaration.visibility == Private
                || (php.kind == RefKind::StaticMethod
                    && !php
                        .receiver
                        .as_deref()
                        .unwrap_or_default()
                        .eq_ignore_ascii_case("static")) =>
            {
                CallDispatch::Direct
            }
            _ => CallDispatch::Virtual,
        }
    }
}
