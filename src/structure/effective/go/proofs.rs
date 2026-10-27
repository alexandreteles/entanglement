use super::*;

pub(super) fn receiver_proofs(
    declaration: &EffectiveDeclaration,
    contract: &EffectiveDeclaration,
    requirements: &BTreeMap<String, String>,
    signatures: &BTreeMap<String, String>,
    is_interface: bool,
) -> Vec<Satisfaction> {
    ["value", "pointer"]
        .into_iter()
        .filter(|receiver| !is_interface || *receiver == "value")
        .filter(|receiver| {
            satisfied(
                declaration,
                receiver,
                requirements,
                signatures,
                is_interface,
            )
        })
        .map(|receiver| Satisfaction {
            contract: contract.unit.clone(),
            receiver: receiver.to_owned(),
            status: CoverageStatus::Complete,
            members: requirements.keys().cloned().collect(),
        })
        .collect()
}

pub(super) fn requirements(
    contract: &EffectiveDeclaration,
    signatures: &BTreeMap<String, String>,
) -> Option<BTreeMap<String, String>> {
    let mut requirements = BTreeMap::new();
    for member in contract.members.iter().filter(|member| member.required) {
        let signature = signatures.get(&member.origin)?;
        if requirements
            .insert(member.name.clone(), signature.clone())
            .is_some_and(|previous| previous != *signature)
        {
            return None;
        }
    }
    Some(requirements)
}

fn satisfied(
    declaration: &EffectiveDeclaration,
    receiver: &str,
    requirements: &BTreeMap<String, String>,
    signatures: &BTreeMap<String, String>,
    is_interface: bool,
) -> bool {
    requirements.iter().all(|(name, signature)| {
        let matches = declaration
            .members
            .iter()
            .filter(|member| {
                member.name == *name
                    && (is_interface || !member.required)
                    && (receiver == "pointer" || member.receiver.as_deref() != Some("pointer"))
            })
            .collect::<Vec<_>>();
        let unique = matches.len() == 1 || is_interface;
        unique
            && !matches.is_empty()
            && matches
                .iter()
                .all(|member| signatures.get(&member.origin) == Some(signature))
    })
}
