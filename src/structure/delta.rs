use super::{graph, model::*};
use std::collections::{BTreeMap, BTreeSet};

/// Compare stable identities with complete paths through both snapshots.
///
/// A lost unit or reduced coverage prevents claims of structural improvement.
/// Renames without a stable identity remain unmatched.
pub fn compare(before: &StructureSnapshot, after: &StructureSnapshot) -> StructuralDelta {
    let mut events = Vec::new();
    for granularity in [UnitKind::Module, UnitKind::Type] {
        for projection in [Projection::Contract, Projection::Full] {
            let previous = graph::analyze_projection(before, granularity, projection);
            let current = graph::analyze_projection(after, granularity, projection);
            compare_projection(before, after, &previous, &current, &mut events);
        }
    }
    events.sort_by(|left, right| {
        (left.granularity, left.projection, &left.kind, &left.units).cmp(&(
            right.granularity,
            right.projection,
            &right.kind,
            &right.units,
        ))
    });
    StructuralDelta { events }
}

fn event(
    kind: &str,
    graph: &GraphAnalysis,
    units: Vec<String>,
    changed: &[Relationship],
) -> StructuralEvent {
    StructuralEvent {
        kind: kind.into(),
        granularity: graph.granularity,
        projection: graph.projection,
        units,
        before: Vec::new(),
        after: Vec::new(),
        changed: changed.to_vec(),
        before_path: None,
        after_path: None,
        before_paths: Vec::new(),
        after_paths: Vec::new(),
    }
}

fn compare_projection(
    before: &StructureSnapshot,
    after: &StructureSnapshot,
    previous: &GraphAnalysis,
    current: &GraphAnalysis,
    events: &mut Vec<StructuralEvent>,
) {
    let previous_units: BTreeSet<_> = previous.units.iter().cloned().collect();
    let current_units: BTreeSet<_> = current.units.iter().cloned().collect();
    let mut ambiguous: BTreeSet<_> = before
        .units
        .iter()
        .chain(&after.units)
        .filter(|unit| {
            before
                .units
                .iter()
                .filter(|candidate| candidate.id == unit.id)
                .count()
                > 1
                || after
                    .units
                    .iter()
                    .filter(|candidate| candidate.id == unit.id)
                    .count()
                    > 1
        })
        .map(|unit| unit.id.clone())
        .collect();
    ambiguous.retain(|unit| previous_units.contains(unit) || current_units.contains(unit));
    ambiguous.extend(rename_candidates(
        before,
        after,
        &previous_units,
        &current_units,
        previous.granularity,
    ));
    let matched: BTreeSet<_> = previous_units
        .intersection(&current_units)
        .filter(|unit| !ambiguous.contains(*unit))
        .cloned()
        .collect();
    let coverage_lost = coverage_lost(before, after)
        || before.units.iter().any(|unit| {
            !current_units.contains(&unit.id)
                && unit.kind == previous.granularity
                && !after.removed_paths.contains(&unit.path)
                && !after.coverage.iter().any(|coverage| {
                    coverage.path == unit.path
                        && matches!(
                            coverage.status,
                            CoverageStatus::Complete | CoverageStatus::Partial
                        )
                })
        });
    let changed = changed_edges(&previous.edges, &current.edges);
    if coverage_lost {
        let mut finding = event(
            "coverage_lost",
            current,
            previous_units.difference(&current_units).cloned().collect(),
            &changed,
        );
        finding.before = before
            .coverage
            .iter()
            .map(|coverage| format!("{}:{:?}", coverage.path, coverage.status))
            .collect();
        finding.after = after
            .coverage
            .iter()
            .map(|coverage| format!("{}:{:?}", coverage.path, coverage.status))
            .collect();
        events.push(finding);
    }
    compare_order(previous, current, &matched, events);
    let improvement_unavailable = coverage_lost || !ambiguous.is_empty();
    compare_dependencies(current, improvement_unavailable, &changed, events);
    compare_reachability(
        previous,
        current,
        &matched,
        improvement_unavailable,
        &changed,
        events,
    );
    let uncertain: BTreeSet<_> = previous
        .cycles
        .iter()
        .chain(&current.cycles)
        .filter(|cycle| cycle.members.iter().any(|unit| ambiguous.contains(unit)))
        .flat_map(|cycle| cycle.members.iter().cloned())
        .chain(ambiguous.iter().cloned())
        .collect();
    if !uncertain.is_empty() {
        let mut finding = event(
            "match_uncertain",
            current,
            uncertain.iter().cloned().collect(),
            &[],
        );
        finding.before = before
            .units
            .iter()
            .filter(|unit| uncertain.contains(&unit.id))
            .map(|unit| {
                format!(
                    "{}:hash={}:shape={}",
                    unit.id,
                    unit.source_hash.as_deref().unwrap_or("?"),
                    unit.structural_fingerprint.as_deref().unwrap_or("?")
                )
            })
            .collect();
        finding.after = after
            .units
            .iter()
            .filter(|unit| uncertain.contains(&unit.id))
            .map(|unit| {
                format!(
                    "{}:hash={}:shape={}",
                    unit.id,
                    unit.source_hash.as_deref().unwrap_or("?"),
                    unit.structural_fingerprint.as_deref().unwrap_or("?")
                )
            })
            .collect();
        events.push(finding);
    }
    compare_cycles(
        previous,
        current,
        coverage_lost,
        &changed,
        &uncertain,
        events,
    );
}

fn rename_candidates(
    before: &StructureSnapshot,
    after: &StructureSnapshot,
    previous_units: &BTreeSet<String>,
    current_units: &BTreeSet<String>,
    granularity: UnitKind,
) -> BTreeSet<String> {
    let previous = before
        .units
        .iter()
        .filter(|unit| unit.kind == granularity && !current_units.contains(&unit.id));
    let current: Vec<_> = after
        .units
        .iter()
        .filter(|unit| unit.kind == granularity && !previous_units.contains(&unit.id))
        .collect();
    previous
        .flat_map(|previous| {
            current
                .iter()
                .filter(move |current| {
                    let same_source = previous.source_hash.as_ref().is_some_and(|hash| {
                        !hash.is_empty() && current.source_hash.as_ref() == Some(hash)
                    }) && (granularity == UnitKind::Module
                        || previous.name == current.name);
                    let same_structure =
                        previous
                            .structural_fingerprint
                            .as_ref()
                            .is_some_and(|fingerprint| {
                                !fingerprint.is_empty()
                                    && current.structural_fingerprint.as_ref() == Some(fingerprint)
                            });
                    same_source || same_structure
                })
                .flat_map(|current| [previous.id.clone(), current.id.clone()])
        })
        .collect()
}

fn ordered_targets(graph: &GraphAnalysis) -> BTreeMap<String, Vec<String>> {
    let mut supports = BTreeMap::<String, BTreeSet<(usize, usize, String)>>::new();
    for edge in graph.edges.iter().filter(|edge| edge.ordered) {
        supports.entry(edge.source.clone()).or_default().extend(
            edge.evidence
                .iter()
                .filter(|evidence| !matches!(evidence.detail.as_str(), "relation" | "embed_ptr"))
                .map(|evidence| (evidence.start, evidence.end, edge.target.clone())),
        );
    }
    supports
        .into_iter()
        .map(|(source, targets)| {
            (
                source,
                targets.into_iter().map(|(_, _, target)| target).collect(),
            )
        })
        .collect()
}

fn compare_order(
    previous: &GraphAnalysis,
    current: &GraphAnalysis,
    matched: &BTreeSet<String>,
    events: &mut Vec<StructuralEvent>,
) {
    let previous_order = ordered_targets(previous);
    let current_order = ordered_targets(current);
    for (source, before) in previous_order
        .iter()
        .filter(|(source, _)| matched.contains(*source))
    {
        let Some(after) = current_order.get(source).filter(|after| *after != before) else {
            continue;
        };
        let mut previous_targets = before.clone();
        previous_targets.sort();
        let mut current_targets = after.clone();
        current_targets.sort();
        if previous_targets != current_targets {
            continue;
        }
        let changed: Vec<_> = previous
            .edges
            .iter()
            .chain(&current.edges)
            .filter(|edge| edge.ordered && edge.source == *source)
            .cloned()
            .collect();
        let mut finding = event(
            "relationship_order_changed",
            current,
            vec![source.clone()],
            &changed,
        );
        finding.before = before.clone();
        finding.after = after.clone();
        finding.before_paths = before
            .iter()
            .filter_map(|target| graph::witness(previous, source, target))
            .collect();
        finding.after_paths = after
            .iter()
            .filter_map(|target| graph::witness(current, source, target))
            .collect();
        finding.before_path = finding.before_paths.first().cloned();
        finding.after_path = finding.after_paths.first().cloned();
        events.push(finding);
    }
}

fn compare_dependencies(
    current: &GraphAnalysis,
    coverage_lost: bool,
    changed: &[Relationship],
    events: &mut Vec<StructuralEvent>,
) {
    for edge in changed {
        let added = current
            .edges
            .iter()
            .any(|candidate| edge_key(candidate) == edge_key(edge));
        if !added && coverage_lost {
            continue;
        }
        let mut finding = event(
            if added {
                "dependency_added"
            } else {
                "dependency_removed"
            },
            current,
            vec![edge.source.clone(), edge.target.clone()],
            std::slice::from_ref(edge),
        );
        if added {
            finding.after = vec![format!("{:?}", edge.kind)];
        } else {
            finding.before = vec![format!("{:?}", edge.kind)];
        }
        events.push(finding);
    }
}

fn compare_reachability(
    previous: &GraphAnalysis,
    current: &GraphAnalysis,
    matched: &BTreeSet<String>,
    coverage_lost: bool,
    changed: &[Relationship],
    events: &mut Vec<StructuralEvent>,
) {
    let previous_paths = reachable_pairs(previous);
    let current_paths = reachable_pairs(current);
    for source in matched {
        for target in matched.iter().filter(|target| *target != source) {
            let key = (source.clone(), target.clone());
            let old_reached = previous_paths.contains(&key);
            let new_reached = current_paths.contains(&key);
            if old_reached == new_reached || (!new_reached && coverage_lost) {
                continue;
            }
            let old = graph::witness(previous, source, target);
            let new = graph::witness(current, source, target);
            let mut finding = event(
                if new.is_some() {
                    "reach_added"
                } else {
                    "reach_removed"
                },
                current,
                vec![source.clone(), target.clone()],
                &path_changes(old.as_ref(), new.as_ref(), changed),
            );
            finding.before_path = old;
            finding.after_path = new;
            events.push(finding);
        }
    }
}

fn coverage_lost(before: &StructureSnapshot, after: &StructureSnapshot) -> bool {
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

fn edge_key(edge: &Relationship) -> (&str, &str, RelationshipKind) {
    (&edge.source, &edge.target, edge.kind)
}

fn changed_edges(before: &[Relationship], after: &[Relationship]) -> Vec<Relationship> {
    before
        .iter()
        .filter(|edge| {
            !after
                .iter()
                .any(|candidate| edge_key(edge) == edge_key(candidate))
        })
        .chain(after.iter().filter(|edge| {
            !before
                .iter()
                .any(|candidate| edge_key(edge) == edge_key(candidate))
        }))
        .cloned()
        .collect()
}

fn reachable_pairs(graph: &GraphAnalysis) -> BTreeSet<(String, String)> {
    graph
        .impacts
        .iter()
        .flat_map(|impact| {
            impact.dependencies.iter().map(|target| {
                (
                    graph.units[impact.unit].clone(),
                    graph.units[*target].clone(),
                )
            })
        })
        .collect()
}

fn path_changes(
    before: Option<&Witness>,
    after: Option<&Witness>,
    changed: &[Relationship],
) -> Vec<Relationship> {
    changed
        .iter()
        .filter(|edge| {
            before.into_iter().chain(after).any(|path| {
                path.units
                    .windows(2)
                    .any(|pair| pair[0] == edge.source && pair[1] == edge.target)
            })
        })
        .cloned()
        .collect()
}

fn compare_cycles(
    previous: &GraphAnalysis,
    current: &GraphAnalysis,
    coverage_lost: bool,
    changed: &[Relationship],
    uncertain: &BTreeSet<String>,
    events: &mut Vec<StructuralEvent>,
) {
    for cycle in current
        .cycles
        .iter()
        .filter(|cycle| cycle.members.iter().all(|unit| !uncertain.contains(unit)))
    {
        let overlaps: Vec<_> = previous
            .cycles
            .iter()
            .filter(|previous| {
                previous
                    .members
                    .iter()
                    .any(|unit| cycle.members.contains(unit))
            })
            .collect();
        let kind = match overlaps.as_slice() {
            [] => "cycle_introduced",
            [previous] if previous.members == cycle.members => "cycle_persistent",
            [previous]
                if previous
                    .members
                    .iter()
                    .all(|unit| cycle.members.contains(unit)) =>
            {
                "cycle_expanded"
            }
            [_] => "cycle_changed",
            _ => "cycle_merged",
        };
        if coverage_lost && kind == "cycle_changed" {
            continue;
        }
        let affected: BTreeSet<_> = cycle
            .members
            .iter()
            .chain(overlaps.iter().flat_map(|cycle| cycle.members.iter()))
            .cloned()
            .collect();
        let relevant: Vec<_> = changed
            .iter()
            .filter(|edge| affected.contains(&edge.source) || affected.contains(&edge.target))
            .cloned()
            .collect();
        let mut finding = event(kind, current, affected.into_iter().collect(), &relevant);
        finding.before = overlaps
            .iter()
            .flat_map(|cycle| cycle.members.clone())
            .collect();
        finding.before.sort();
        finding.before.dedup();
        finding.after = cycle.members.clone();
        finding.before_path = overlaps.first().map(|cycle| cycle.witness.clone());
        finding.after_path = Some(cycle.witness.clone());
        events.push(finding);
    }
    if coverage_lost {
        return;
    }
    compare_cycle_removals(previous, current, changed, uncertain, events);
}

fn compare_cycle_removals(
    previous: &GraphAnalysis,
    current: &GraphAnalysis,
    changed: &[Relationship],
    uncertain: &BTreeSet<String>,
    events: &mut Vec<StructuralEvent>,
) {
    for cycle in previous
        .cycles
        .iter()
        .filter(|cycle| cycle.members.iter().all(|unit| !uncertain.contains(unit)))
    {
        let overlaps: Vec<_> = current
            .cycles
            .iter()
            .filter(|current| {
                current
                    .members
                    .iter()
                    .any(|unit| cycle.members.contains(unit))
            })
            .collect();
        let kind = match overlaps.len() {
            0 => "cycle_resolved",
            2.. => "cycle_split",
            _ => continue,
        };
        let mut finding = event(kind, current, cycle.members.clone(), changed);
        finding.before = cycle.members.clone();
        finding.after = overlaps
            .iter()
            .flat_map(|cycle| cycle.members.clone())
            .collect();
        finding.after.sort();
        finding.after.dedup();
        finding.before_path = Some(cycle.witness.clone());
        finding.after_path = overlaps.first().map(|cycle| cycle.witness.clone());
        events.push(finding);
    }
}
