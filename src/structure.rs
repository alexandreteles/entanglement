//! Analyze source dependencies without a composite quality score.

pub(crate) mod delta;
pub(crate) mod effective;
pub(crate) mod extract;
pub(crate) mod graph;
pub(crate) mod model;
pub(crate) mod resources;
pub(crate) mod types;

use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

use crate::model::FileFacts;

/// Structural results for one immutable source state.
#[derive(Clone, Debug, Serialize)]
pub struct StructuralState {
    #[serde(flatten)]
    pub snapshot: model::StructureSnapshot,
    pub graphs: Vec<model::GraphAnalysis>,
    pub effective: Vec<effective::EffectiveDeclaration>,
}

/// A source state with optional comparison evidence.
#[derive(Clone, Debug, Serialize)]
pub struct StructuralReport {
    #[serde(flatten)]
    pub state: StructuralState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<StructuralSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<model::StructuralDelta>,
}

/// Pre-change values for review. This is not a reusable full snapshot.
#[derive(Clone, Debug, Serialize)]
pub struct StructuralSummary {
    pub summary: bool,
    pub scope: String,
    pub coverage: Vec<model::Coverage>,
    pub graphs: Vec<GraphSummary>,
}

/// Counts, cycle memberships, and PC for one pre-change graph.
#[derive(Clone, Debug, Serialize)]
pub struct GraphSummary {
    pub granularity: model::UnitKind,
    pub projection: model::Projection,
    pub units: usize,
    pub edges: usize,
    pub boundary: usize,
    pub cycles: Vec<model::Cycle>,
    pub pc: model::PropagationCost,
}

/// Build structural results from resolved source facts.
pub(crate) fn analyze(
    facts: &[FileFacts],
    scope: &str,
    source_snapshot: &crate::snapshot::Snapshot,
) -> StructuralReport {
    let snapshot = extract::snapshot(facts, scope, source_snapshot);
    let graphs = graph::analyze(&snapshot);
    let effective = effective::analyze(facts, &snapshot, source_snapshot);
    StructuralReport {
        state: StructuralState {
            snapshot,
            graphs,
            effective,
        },
        before: None,
        delta: None,
    }
}

/// Compare complete source states, including effects on unchanged files.
pub(crate) fn compare(before: StructuralReport, mut after: StructuralReport) -> StructuralReport {
    let mut delta = delta::compare(&before.state.snapshot, &after.state.snapshot);
    let coverage_lost = delta
        .events
        .iter()
        .any(|event| event.kind == "coverage_lost");
    let match_uncertain = delta
        .events
        .iter()
        .any(|event| event.kind == "match_uncertain");
    let mut effective_events = effective::compare(&before.state.effective, &after.state.effective);
    attribute_effective(
        &mut effective_events,
        &delta.events,
        &before.state,
        &after.state,
    );
    for event in &mut effective_events {
        if coverage_lost {
            event.kind = "effective_coverage".to_owned();
        } else if match_uncertain {
            event.kind = "effective_match".to_owned();
        }
    }
    delta.events.extend(effective_events);
    after.delta = Some(delta);
    after.before = Some(StructuralSummary {
        summary: true,
        scope: before.state.snapshot.scope,
        coverage: before.state.snapshot.coverage,
        graphs: before
            .state
            .graphs
            .into_iter()
            .map(|graph| GraphSummary {
                granularity: graph.granularity,
                projection: graph.projection,
                units: graph.units.len(),
                edges: graph.edges.len(),
                boundary: graph.boundary.len(),
                cycles: graph.cycles,
                pc: graph.pc,
            })
            .collect(),
    });
    after
}

fn attribute_effective(
    events: &mut [model::StructuralEvent],
    changes: &[model::StructuralEvent],
    before: &StructuralState,
    after: &StructuralState,
) {
    let mut relationships = BTreeMap::new();
    for relationship in changes.iter().flat_map(|event| &event.changed) {
        let key = (
            relationship.source.clone(),
            relationship.target.clone(),
            relationship.kind,
            relationship.contract,
            relationship.resolution,
        );
        let entry = relationships.entry(key).or_insert_with(|| {
            let mut relationship = relationship.clone();
            relationship.evidence.clear();
            relationship
        });
        entry.evidence.extend(relationship.evidence.iter().cloned());
    }
    for relationship in relationships.values_mut() {
        relationship.evidence.sort();
        relationship.evidence.dedup();
    }
    for event in events {
        let units = contract_units(event, before, after);
        event.changed = relationships
            .values()
            .filter(|edge| units.contains(&edge.source))
            .cloned()
            .collect();
    }
}

fn contract_units(
    event: &model::StructuralEvent,
    before: &StructuralState,
    after: &StructuralState,
) -> BTreeSet<String> {
    let mut units: BTreeSet<String> = event
        .units
        .iter()
        .chain(
            event
                .before_paths
                .iter()
                .chain(&event.after_paths)
                .flat_map(|path| &path.units),
        )
        .cloned()
        .collect();
    for graph in before.graphs.iter().chain(&after.graphs).filter(|graph| {
        graph.granularity == model::UnitKind::Type
            && graph.projection == model::Projection::Contract
    }) {
        units.extend(
            graph
                .impacts
                .iter()
                .filter(|impact| event.units.contains(&graph.units[impact.unit]))
                .flat_map(|impact| {
                    impact
                        .dependencies
                        .iter()
                        .map(|index| graph.units[*index].clone())
                }),
        );
    }
    units
}
