use super::super::model::{RelationshipKind, StructureSnapshot};
use super::{EffectiveMember, python};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

mod promotion;
mod selection;

use promotion::{ambiguous_promotions, promote_pointer_receivers};
use selection::{ambiguous_members, script_choice};

/// Language rules for member precedence and receiver promotion.
pub(super) enum Policy {
    Python,
    Go,
    GoInterface,
    Rust,
    TypeContracts,
    ScriptClass,
    Resolved,
}

impl Policy {
    pub(super) fn for_declaration(language: &str, kind: &str) -> Self {
        if language.contains("python") {
            Self::Python
        } else if language.contains("go") {
            if kind == "interface" {
                Self::GoInterface
            } else {
                Self::Go
            }
        } else if language.contains("rust") {
            Self::Rust
        } else if language.contains("typescript")
            || language.ends_with(".ts")
            || language.contains("tsx")
            || language.contains("astro")
        {
            if matches!(kind, "interface" | "alias") {
                Self::TypeContracts
            } else {
                Self::ScriptClass
            }
        } else if language.contains("javascript")
            || language.ends_with(".js")
            || language.contains("jsx")
        {
            Self::ScriptClass
        } else {
            Self::Resolved
        }
    }

    pub(super) fn order(
        &self,
        unit: &str,
        parents: &BTreeMap<String, Vec<String>>,
        paths: &BTreeMap<String, Vec<String>>,
    ) -> Result<Vec<String>, String> {
        match self {
            Self::Python => python::order(unit, parents),
            _ => Ok(paths.keys().cloned().collect()),
        }
    }

    pub(super) fn choose(
        &self,
        choices: Vec<EffectiveMember>,
        own: &[EffectiveMember],
        name: &str,
        order: &[String],
    ) -> Vec<EffectiveMember> {
        if matches!(self, Self::Rust | Self::TypeContracts | Self::GoInterface) {
            return choices;
        }
        if matches!(self, Self::ScriptClass) {
            return script_choice(choices, own, name);
        }
        if matches!(self, Self::Python) {
            return order
                .iter()
                .find_map(|origin| {
                    choices
                        .iter()
                        .find(|choice| choice.provenance.last() == Some(origin))
                        .cloned()
                })
                .into_iter()
                .collect();
        }
        let member_kind = choices
            .first()
            .and_then(|member| member.member_kind.as_ref());
        if let Some(member) = own
            .iter()
            .find(|member| member.name == name && member.member_kind.as_ref() == member_kind)
        {
            return vec![member.clone()];
        }
        let minimum = choices
            .first()
            .map(|choice| choice.provenance.len())
            .unwrap_or(0);
        choices
            .into_iter()
            .filter(|choice| choice.provenance.len() == minimum)
            .collect()
    }

    pub(super) fn inherited_contracts(&self, choices: &[EffectiveMember]) -> BTreeSet<String> {
        if matches!(self, Self::Rust | Self::TypeContracts | Self::GoInterface) {
            return BTreeSet::new();
        }
        choices
            .iter()
            .filter(|member| member.required)
            .flat_map(|member| member.contracts.iter().cloned())
            .collect()
    }

    pub(super) fn finish(
        &self,
        members: &mut [EffectiveMember],
        snapshot: &StructureSnapshot,
    ) -> Vec<String> {
        match self {
            Self::Python => vec!["runtime_structure".to_owned()],
            Self::Go => {
                promote_pointer_receivers(members, snapshot);
                let mut obligations = ambiguous_promotions(members, snapshot);
                obligations.push("structural_satisfaction".to_owned());
                obligations
            }
            Self::Rust => ambiguous_members(members, "qualified_selection"),
            Self::TypeContracts => vec!["structural_compat".to_owned()],
            Self::GoInterface => vec!["structural_satisfaction".to_owned()],
            Self::ScriptClass => ambiguous_members(members, "selection_unavailable")
                .into_iter()
                .chain(
                    members
                        .iter()
                        .filter(|member| {
                            member.member_kind.is_none()
                                && member
                                    .evidence
                                    .iter()
                                    .any(|evidence| evidence.detail == "member")
                        })
                        .map(|member| format!("selection_unavailable:{}", member.name)),
                )
                .collect(),
            Self::Resolved => Vec::new(),
        }
    }
}
