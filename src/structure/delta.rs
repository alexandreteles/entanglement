use super::{graph, model::*};
use std::collections::{BTreeMap, BTreeSet};

#[path = "delta/coverage.rs"]
mod coverage;
#[path = "delta/cycles.rs"]
mod cycles;
#[path = "delta/dependencies.rs"]
mod dependencies;
#[path = "delta/matching.rs"]
mod matching;
#[path = "delta/order.rs"]
mod order;

use coverage::coverage_lost;
use cycles::compare_cycles;
use dependencies::{changed_edges, compare_dependencies, compare_reachability};
use matching::rename_candidates;
use order::compare_order;

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
