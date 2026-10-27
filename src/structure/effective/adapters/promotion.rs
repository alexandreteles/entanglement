use super::*;

pub(super) fn ambiguous_promotions(
    members: &mut [EffectiveMember],
    snapshot: &StructureSnapshot,
) -> Vec<String> {
    let mut adjacency = BTreeMap::<&str, BTreeSet<&str>>::new();
    for edge in snapshot.relationships.iter().filter(|edge| {
        edge.kind == RelationshipKind::Compose
            && edge.resolution == super::super::super::model::ResolutionStatus::Resolved
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

pub(super) fn promote_pointer_receivers(
    members: &mut [EffectiveMember],
    snapshot: &StructureSnapshot,
) {
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
