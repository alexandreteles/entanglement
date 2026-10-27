use super::*;

pub(super) fn script_choice(
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

pub(super) fn ambiguous_members(members: &[EffectiveMember], code: &str) -> Vec<String> {
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

impl Policy {
    pub(in crate::structure::effective) fn conflicts(
        &self,
        members: &mut [EffectiveMember],
    ) -> Vec<String> {
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
}
