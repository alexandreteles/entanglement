use super::super::model::{RelationshipKind, StructureSnapshot};
use super::{EffectiveMember, python};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

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

    pub(super) fn conflicts(&self, members: &mut [EffectiveMember]) -> Vec<String> {
        if matches!(self, Self::Rust | Self::TypeContracts | Self::GoInterface) {
            return Vec::new();
        }
        if matches!(self, Self::ScriptClass) {
            for member in members.iter_mut().filter(|member| {
                member.member_kind.is_none()
                    && member
                        .evidence
                        .iter()
                        .any(|evidence| evidence.detail == "member")
            }) {
                member.body = None;
            }
        }
        let mut bodies = BTreeMap::<(String, Option<String>), BTreeSet<String>>::new();
        for member in members.iter() {
            if let Some(body) = &member.body {
                bodies
                    .entry((member.name.clone(), member.member_kind.clone()))
                    .or_default()
                    .insert(body.clone());
            }
        }
        let conflicts: BTreeSet<_> = bodies
            .into_iter()
            .filter(|(_, origins)| origins.len() > 1)
            .map(|(name, _)| name)
            .collect();
        for member in members {
            if conflicts.contains(&(member.name.clone(), member.member_kind.clone())) {
                member.body = None;
            }
        }
        if matches!(self, Self::ScriptClass) {
            return Vec::new();
        }
        conflicts
            .into_iter()
            .map(|(name, _)| format!("member:{name}"))
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

fn script_choice(
    choices: Vec<EffectiveMember>,
    own: &[EffectiveMember],
    name: &str,
) -> Vec<EffectiveMember> {
    let Some(kind) = choices
        .first()
        .and_then(|member| member.member_kind.as_deref())
    else {
        return choices;
    };
    if kind == "constructor" {
        return own
            .iter()
            .filter(|member| {
                member.name == name && member.member_kind.as_deref() == Some("constructor")
            })
            .cloned()
            .collect();
    }
    let is_static = kind.starts_with("static");
    let own_property: Vec<_> = own
        .iter()
        .filter(|member| {
            member.name == name
                && member
                    .member_kind
                    .as_deref()
                    .is_some_and(|kind| kind.starts_with("static") == is_static)
        })
        .collect();
    if !own_property.is_empty() {
        return own_property
            .into_iter()
            .filter(|member| member.member_kind.as_deref() == Some(kind))
            .cloned()
            .collect();
    }
    let minimum = choices
        .iter()
        .map(|member| member.provenance.len())
        .min()
        .unwrap_or(0);
    choices
        .into_iter()
        .filter(|member| member.provenance.len() == minimum)
        .collect()
}

fn ambiguous_members(members: &[EffectiveMember], code: &str) -> Vec<String> {
    let mut origins = BTreeMap::<(&str, Option<&str>), BTreeSet<&str>>::new();
    for member in members {
        origins
            .entry((&member.name, member.member_kind.as_deref()))
            .or_default()
            .insert(&member.origin);
    }
    origins
        .into_iter()
        .filter(|(_, origins)| origins.len() > 1)
        .map(|((name, _), _)| format!("{code}:{name}"))
        .collect()
}

fn ambiguous_promotions(
    members: &mut [EffectiveMember],
    snapshot: &StructureSnapshot,
) -> Vec<String> {
    let mut adjacency = BTreeMap::<&str, BTreeSet<&str>>::new();
    for edge in snapshot.relationships.iter().filter(|edge| {
        edge.kind == RelationshipKind::Compose
            && edge.resolution == super::super::model::ResolutionStatus::Resolved
    }) {
        adjacency
            .entry(&edge.source)
            .or_default()
            .insert(&edge.target);
    }
    let mut obligations = Vec::new();
    for member in members
        .iter_mut()
        .filter(|member| member.provenance.len() > 1)
    {
        let source = member.provenance.first().unwrap();
        let target = member.provenance.last().unwrap();
        if composition_paths(source, target, &adjacency) > 1 {
            member.body = None;
            obligations.push(format!("promotion_ambiguous:{}", member.name));
        }
    }
    obligations
}

fn composition_paths(source: &str, target: &str, adjacency: &BTreeMap<&str, BTreeSet<&str>>) -> u8 {
    let mut reached = BTreeMap::from([(source, (0, 1u8))]);
    let mut pending = VecDeque::from([source]);
    while let Some(current) = pending.pop_front() {
        let (depth, count) = reached[current];
        for next in adjacency.get(current).into_iter().flatten() {
            match reached.get(next).copied() {
                None => {
                    reached.insert(next, (depth + 1, count));
                    pending.push_back(next);
                }
                Some((previous_depth, previous_count)) if previous_depth == depth + 1 => {
                    reached.insert(next, (previous_depth, (previous_count + count).min(2)));
                }
                _ => {}
            }
        }
    }
    reached.get(target).map_or(0, |(_, count)| *count)
}

fn promote_pointer_receivers(members: &mut [EffectiveMember], snapshot: &StructureSnapshot) {
    for member in members
        .iter_mut()
        .filter(|member| member.receiver.as_deref() == Some("pointer"))
    {
        let embedded_pointer = member.provenance.windows(2).any(|pair| {
            snapshot
                .relationships
                .iter()
                .filter(|edge| {
                    edge.source == pair[0]
                        && edge.target == pair[1]
                        && edge.kind == RelationshipKind::Compose
                })
                .flat_map(|edge| &edge.evidence)
                .any(|evidence| evidence.detail == "embed_ptr")
        });
        if embedded_pointer {
            member.receiver = Some("value".to_owned());
        }
    }
}
