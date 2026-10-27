use super::*;

pub(super) fn parent_map(snapshot: &StructureSnapshot) -> BTreeMap<String, Vec<String>> {
    let mut edges = snapshot
        .relationships
        .iter()
        .filter(|edge| {
            edge.resolution == ResolutionStatus::Resolved
                && matches!(
                    edge.kind,
                    RelationshipKind::Inherit
                        | RelationshipKind::Compose
                        | RelationshipKind::Implement
                )
                && edge.source.starts_with("type:")
                && edge.target.starts_with("type:")
        })
        .collect::<Vec<_>>();
    edges.sort_by_key(|edge| {
        edge.evidence
            .iter()
            .filter(|evidence| !matches!(evidence.detail.as_str(), "relation" | "embed_ptr"))
            .map(|evidence| evidence.start)
            .min()
            .unwrap_or(0)
    });
    let mut parents = BTreeMap::<String, Vec<String>>::new();
    for edge in edges {
        let list = parents.entry(edge.source.clone()).or_default();
        if !list.contains(&edge.target) {
            list.push(edge.target.clone());
        }
    }
    parents
}

pub(super) fn provenance(
    unit: &str,
    parents: &BTreeMap<String, Vec<String>>,
) -> BTreeMap<String, Vec<String>> {
    let mut paths = BTreeMap::from([(unit.to_owned(), vec![unit.to_owned()])]);
    let mut pending = VecDeque::from([unit.to_owned()]);
    while let Some(current) = pending.pop_front() {
        for parent in parents.get(&current).into_iter().flatten() {
            if paths.contains_key(parent) {
                continue;
            }
            let mut path = paths[&current].clone();
            path.push(parent.clone());
            paths.insert(parent.clone(), path);
            pending.push_back(parent.clone());
        }
    }
    paths
}
