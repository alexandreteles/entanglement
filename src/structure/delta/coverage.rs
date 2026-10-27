use super::*;

pub(super) fn coverage_lost(before: &StructureSnapshot, after: &StructureSnapshot) -> bool {
    let parse_loss = before.coverage.iter().any(|previous| {
        if after.removed_paths.contains(&previous.path) {
            return false;
        }
        let current = after
            .coverage
            .iter()
            .find(|current| current.path == previous.path && current.language == previous.language);
        match (previous.status, current.map(|coverage| coverage.status)) {
            (_, None) => true,
            (
                CoverageStatus::Complete,
                Some(
                    CoverageStatus::Partial
                    | CoverageStatus::Unavailable
                    | CoverageStatus::Inapplicable,
                ),
            ) => true,
            (
                CoverageStatus::Partial,
                Some(CoverageStatus::Unavailable | CoverageStatus::Inapplicable),
            ) => true,
            _ => current.is_some_and(|current| {
                current
                    .limitations
                    .iter()
                    .any(|limitation| !previous.limitations.contains(limitation))
            }),
        }
    });
    let unresolved = |snapshot: &StructureSnapshot| {
        let mut counts = BTreeMap::<String, usize>::new();
        for edge in &snapshot.relationships {
            if matches!(
                edge.resolution,
                ResolutionStatus::Unresolved | ResolutionStatus::Ambiguous
            ) {
                *counts.entry(edge.source.clone()).or_default() += edge.evidence.len().max(1);
            }
        }
        counts
    };
    let previous_unresolved = unresolved(before);
    let current_unresolved = unresolved(after);
    let previous_resolved = support_keys(before, ResolutionStatus::Resolved);
    let current_resolved = support_keys(after, ResolutionStatus::Resolved);
    let current_external = support_keys(after, ResolutionStatus::External);
    let lost_internal_target = previous_resolved
        .difference(&current_resolved)
        .any(|support| current_external.contains(support));
    parse_loss
        || lost_internal_target
        || current_unresolved
            .iter()
            .any(|(source, count)| *count > previous_unresolved.get(source).copied().unwrap_or(0))
}

fn support_keys(
    snapshot: &StructureSnapshot,
    resolution: ResolutionStatus,
) -> BTreeSet<(String, RelationshipKind, String, String)> {
    snapshot
        .relationships
        .iter()
        .filter(|edge| edge.resolution == resolution)
        .flat_map(|edge| {
            edge.evidence
                .iter()
                .filter(|evidence| {
                    !matches!(evidence.detail.as_str(), "relation" | "embed_ptr")
                        && !(edge.kind == RelationshipKind::Resource && evidence.detail == "url")
                })
                .map(|evidence| {
                    (
                        edge.source.clone(),
                        edge.kind,
                        evidence.owner.clone(),
                        evidence.detail.clone(),
                    )
                })
        })
        .collect()
}
