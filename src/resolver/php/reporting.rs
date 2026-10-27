use super::*;

pub(in crate::resolver) fn structural_members(
    graph: &ProgramGraph<'_>,
    snapshot: &Snapshot,
) -> Vec<super::super::StructuralMemberSelection> {
    let index = Index::new(graph, snapshot);
    let names = index
        .candidates
        .iter()
        .filter(|candidate| candidate.owner.is_some())
        .map(|candidate| candidate.symbol.name.clone())
        .chain(
            graph
                .files()
                .iter()
                .flat_map(|fact| fact.semantic())
                .filter_map(|store| store.semantic.php.as_ref())
                .flat_map(|php| &php.traits)
                .flat_map(|usage| &usage.adaptations)
                .filter_map(|adaptation| adaptation.alias.clone()),
        )
        .collect::<std::collections::BTreeSet<_>>();
    index
        .candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.php.kind == PhpSymbolKind::Class)
        .flat_map(|(owner, candidate)| {
            names
                .iter()
                .flat_map(|name| {
                    let index = &index;
                    [
                        PhpSymbolKind::Method,
                        PhpSymbolKind::Property,
                        PhpSymbolKind::Constant,
                    ]
                    .into_iter()
                    .filter_map(move |kind| {
                        let candidates = index.structural_member_candidates(owner, kind, name);
                        let members = candidates.members;
                        (!members.is_empty()).then(|| super::super::StructuralMemberSelection {
                            owner: candidate.symbol.clone(),
                            name: name.clone(),
                            obligations: candidates
                                .version_dependent
                                .then(|| format!("php_version:{name}"))
                                .into_iter()
                                .collect(),
                            visibility: members
                                .iter()
                                .map(|member| {
                                    (
                                        index.candidates[member.candidate].symbol.clone(),
                                        format!(
                                            "{:?}",
                                            member.visibility.unwrap_or(
                                                index.candidates[member.candidate]
                                                    .declaration
                                                    .visibility
                                            )
                                        )
                                        .to_lowercase(),
                                    )
                                })
                                .collect(),
                            origins: members
                                .into_iter()
                                .map(|member| index.candidates[member.candidate].symbol.clone())
                                .collect(),
                        })
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Resolve trait uses and parent declarations with the existing PHP index.
pub(in crate::resolver) fn structural_type_relationships(
    graph: &ProgramGraph<'_>,
    snapshot: &Snapshot,
) -> Vec<super::super::StructuralTypeRelationship> {
    use crate::structure::model::RelationshipKind;
    let index = Index::new(graph, snapshot);
    index
        .candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.php.kind == PhpSymbolKind::Class)
        .flat_map(|(owner, candidate)| {
            let traits = index.trait_uses(owner).flat_map(|usage| {
                usage
                    .traits
                    .iter()
                    .map(move |name| (name, usage.start_byte, RelationshipKind::Compose))
            });
            let parents = candidate
                .declaration
                .supertypes
                .iter()
                .map(|name| (name, candidate.symbol.start_byte, RelationshipKind::Inherit));
            traits
                .chain(parents)
                .map(|(name, start, kind)| {
                    let candidates = index.lookup(candidate.site, name, PhpSymbolKind::Class, None);
                    let kind = if kind == RelationshipKind::Inherit
                        && !candidate.php.is_interface
                        && candidates.len() == 1
                        && index.candidates[candidates[0]].php.is_interface
                    {
                        RelationshipKind::Implement
                    } else {
                        kind
                    };
                    let targets = candidates
                        .into_iter()
                        .map(|target| index.candidates[target].symbol.clone())
                        .collect::<Vec<_>>();
                    let target = match targets.as_slice() {
                        [] => Resolution::Unresolved,
                        [target] => Resolution::Exact(target.clone()),
                        _ => Resolution::Ambiguous(targets),
                    };
                    super::super::StructuralTypeRelationship {
                        owner: candidate.symbol.clone(),
                        target,
                        name: name.clone(),
                        kind,
                        start,
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect()
}
