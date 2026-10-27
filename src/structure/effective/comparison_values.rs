use super::*;

pub(super) fn semantic_signature(declaration: &EffectiveDeclaration) -> Vec<String> {
    declaration
        .members
        .iter()
        .map(|member| {
            format!(
                "{:?}",
                (
                    &member.name,
                    member.required,
                    member.optional,
                    &member.origin,
                    &member.body,
                    &member.selection,
                    &member.receiver,
                    &member.member_kind,
                    &member.contracts,
                    &member.visibility
                )
            )
        })
        .chain(declaration.types.iter().map(|structure| {
            format!(
                "{:?}",
                (
                    structure.written.semantic_key(),
                    structure
                        .expanded
                        .as_ref()
                        .map(|expanded| expanded.semantic_key()),
                    &structure.origin
                )
            )
        }))
        .chain(declaration.satisfies.iter().map(|proof| {
            format!(
                "satisfies:{}:{}:{:?}:{}",
                proof.receiver,
                proof.contract,
                proof.status,
                proof.members.join(",")
            )
        }))
        .chain(declaration.obligations.iter().cloned())
        .chain(declaration.conflicts.iter().cloned())
        .collect()
}

pub(super) fn effective_values(declaration: &EffectiveDeclaration) -> Vec<String> {
    declaration
        .members
        .iter()
        .map(|member| {
            format!(
                "{}:{}:{}:{}:{}:{}",
                member.name,
                if member.optional {
                    "optional"
                } else if member.required {
                    "req"
                } else {
                    "provided"
                },
                member.member_kind.as_deref().unwrap_or("member"),
                member.visibility,
                member.selection.as_deref().unwrap_or("known"),
                member.body.as_deref().unwrap_or("?")
            )
        })
        .chain(declaration.types.iter().map(|structure| {
            format!(
                "type:{}=>{}",
                structure.written.text,
                structure
                    .expanded
                    .as_ref()
                    .map_or("?", |expanded| expanded.text.as_str())
            )
        }))
        .chain(declaration.satisfies.iter().map(|proof| {
            format!(
                "satisfies:{}:{}:{:?}:{}",
                proof.receiver,
                proof.contract,
                proof.status,
                proof.members.join(",")
            )
        }))
        .chain(declaration.obligations.iter().cloned())
        .chain(declaration.conflicts.iter().cloned())
        .collect()
}
