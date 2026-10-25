//! Select PHP members with visibility, inheritance, and trait rules.

use super::*;

mod access;
mod receivers;
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

    pub(super) fn trait_uses(
        &self,
        class: usize,
    ) -> impl Iterator<Item = &crate::graph::PhpTraitUse> {
        let candidate = &self.candidates[class];
        self.facts(candidate.site.file, candidate.site.context)
            .into_iter()
            .flat_map(|facts| &facts.traits)
            .filter(|usage| usage.owner.eq_ignore_ascii_case(&candidate.name))
    }

    pub(super) fn traits(&self, class: usize) -> Vec<usize> {
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
}
