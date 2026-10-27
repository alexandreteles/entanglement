#[path = "../src/structure/delta.rs"]
mod delta;
#[path = "../src/structure/graph.rs"]
mod graph;
#[path = "../src/structure/model.rs"]
mod model;

use model::*;

fn snapshot(names: &[&str], edges: &[(&str, &str)]) -> StructureSnapshot {
    StructureSnapshot {
        scope: "repository".into(),
        removed_paths: Vec::new(),
        units: names
            .iter()
            .map(|name| Unit {
                id: (*name).into(),
                name: (*name).into(),
                source_hash: None,
                structural_fingerprint: None,
                path: format!("{name}.rs"),
                kind: UnitKind::Module,
            })
            .collect(),
        relationships: edges
            .iter()
            .map(|(source, target)| Relationship {
                ordered: false,
                source: (*source).into(),
                target: (*target).into(),
                kind: RelationshipKind::Import,
                contract: true,
                resolution: ResolutionStatus::Resolved,
                evidence: vec![Evidence {
                    path: format!("{source}.rs"),
                    start: 0,
                    end: 1,
                    owner: (*source).into(),
                    detail: "import".into(),
                }],
            })
            .collect(),
        coverage: Vec::new(),
    }
}

#[path = "structural_graph/cost.rs"]
mod cost;
#[path = "structural_graph/coverage.rs"]
mod coverage;
#[path = "structural_graph/cycles.rs"]
mod cycles;
#[path = "structural_graph/identity.rs"]
mod identity;
#[path = "structural_graph/ordering.rs"]
mod ordering;
