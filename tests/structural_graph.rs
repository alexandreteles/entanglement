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

#[test]
fn exact_reflexive_propagation_cost() {
    for (names, edges, numerator, denominator) in [
        (vec![], vec![], 0, 0),
        (vec!["a", "b", "c"], vec![], 3, 9),
        (vec!["a", "b", "c"], vec![("a", "b"), ("b", "c")], 6, 9),
        (
            vec!["a", "b", "c"],
            vec![("a", "b"), ("b", "c"), ("c", "a")],
            9,
            9,
        ),
    ] {
        let result = graph::analyze_projection(
            &snapshot(&names, &edges),
            UnitKind::Module,
            Projection::Full,
        );
        assert_eq!(
            (result.pc.numerator, result.pc.denominator),
            (numerator, denominator)
        );
        assert_eq!(result.pc.value.is_none(), names.is_empty());
    }
}

#[test]
fn duplicates_keep_evidence_and_cycles_have_witnesses() {
    let mut state = snapshot(&["a", "b"], &[("a", "b"), ("b", "a"), ("a", "a")]);
    let mut repeated = state.relationships[0].clone();
    repeated.evidence[0].start = 9;
    state.relationships.push(repeated);
    let result = graph::analyze_projection(&state, UnitKind::Module, Projection::Full);
    assert_eq!(result.edges.len(), 2);
    assert_eq!(result.edges[0].evidence.len(), 2);
    assert_eq!(result.cycles.len(), 1);
    assert_eq!(
        result.cycles[0].witness.units.first(),
        result.cycles[0].witness.units.last()
    );
    assert_eq!(result.impacts[0].dependencies.len(), 1);
    assert_eq!(result.impacts[0].dependents.len(), 1);
}

#[test]
fn new_intermediary_preserves_matched_endpoint_comparison() {
    let before = snapshot(&["a", "b"], &[]);
    let after = snapshot(
        &["a", "adapter", "b"],
        &[("a", "adapter"), ("adapter", "b")],
    );
    let change = delta::compare(&before, &after);
    assert!(change.events.iter().any(|event| event.kind == "reach_added"
        && event.units == ["a", "b"]
        && event.after_path.as_ref().unwrap().units == ["a", "adapter", "b"]));
}

#[test]
fn cycle_merge_and_split_are_membership_events() {
    let separate = snapshot(
        &["a", "b", "c", "d"],
        &[("a", "b"), ("b", "a"), ("c", "d"), ("d", "c")],
    );
    let merged = snapshot(
        &["a", "b", "c", "d"],
        &[
            ("a", "b"),
            ("b", "a"),
            ("c", "d"),
            ("d", "c"),
            ("b", "c"),
            ("d", "a"),
        ],
    );
    assert!(
        delta::compare(&separate, &merged)
            .events
            .iter()
            .any(|event| event.kind == "cycle_merged")
    );
    assert!(
        delta::compare(&merged, &separate)
            .events
            .iter()
            .any(|event| event.kind == "cycle_split")
    );
}

#[test]
fn coverage_loss_does_not_resolve_a_cycle() {
    let before = snapshot(&["a", "b"], &[("a", "b"), ("b", "a")]);
    let after = snapshot(&["a"], &[]);
    let change = delta::compare(&before, &after);
    assert!(
        !change
            .events
            .iter()
            .any(|event| event.kind == "cycle_resolved")
    );
    assert!(
        change
            .events
            .iter()
            .any(|event| event.kind == "coverage_lost")
    );
}

#[test]
fn weighted_components_and_projection_universes_are_separate() {
    let mut state = snapshot(
        &["a", "b", "c", "d"],
        &[("a", "b"), ("b", "a"), ("b", "c"), ("c", "d"), ("d", "c")],
    );
    state.units.push(Unit {
        id: "type".into(),
        name: "Type".into(),
        source_hash: None,
        structural_fingerprint: None,
        kind: UnitKind::Type,
        path: "a.rs".into(),
    });
    state.relationships[2].contract = false;
    let full = graph::analyze_projection(&state, UnitKind::Module, Projection::Full);
    let contract = graph::analyze_projection(&state, UnitKind::Module, Projection::Contract);
    let types = graph::analyze_projection(&state, UnitKind::Type, Projection::Full);
    assert_eq!((full.pc.numerator, full.pc.denominator), (12, 16));
    assert_eq!(contract.pc.numerator, 8);
    assert_eq!((types.pc.numerator, types.pc.denominator), (1, 1));
}

#[test]
fn explicit_deletion_can_resolve_a_cycle() {
    let before = snapshot(&["a", "b"], &[("a", "b"), ("b", "a")]);
    let mut after = snapshot(&["a"], &[]);
    after.removed_paths.push("b.rs".into());
    let change = delta::compare(&before, &after);
    assert!(
        change
            .events
            .iter()
            .any(|event| event.kind == "cycle_resolved")
    );
    assert!(
        !change
            .events
            .iter()
            .any(|event| event.kind == "coverage_lost")
    );
}

#[test]
fn resolution_loss_blocks_false_improvement() {
    let before = snapshot(&["a", "b"], &[("a", "b"), ("b", "a")]);
    let mut after = before.clone();
    after.relationships[0].resolution = ResolutionStatus::Unresolved;
    after.relationships[0].target = "written_name".into();
    let change = delta::compare(&before, &after);
    assert!(
        change
            .events
            .iter()
            .any(|event| event.kind == "coverage_lost")
    );
    assert!(
        !change
            .events
            .iter()
            .any(|event| event.kind == "cycle_resolved"
                || event.kind == "dependency_removed"
                || event.kind == "reach_removed")
    );
}

#[test]
fn repeated_reference_removal_keeps_dependency() {
    let mut before = snapshot(&["a", "b"], &[("a", "b")]);
    let mut reference = before.relationships[0].clone();
    reference.evidence[0].start = 10;
    before.relationships.push(reference);
    let after = snapshot(&["a", "b"], &[("a", "b")]);
    assert!(delta::compare(&before, &after).events.is_empty());
}

#[test]
fn unchanged_source_ranges_do_not_change_structural_results() {
    let before = snapshot(&["a", "b"], &[("a", "b"), ("b", "a")]);
    let mut after = before.clone();
    for edge in &mut after.relationships {
        edge.evidence[0].start += 20;
        edge.evidence[0].end += 20;
    }
    assert!(
        delta::compare(&before, &after)
            .events
            .iter()
            .all(|event| event.kind == "cycle_persistent")
    );
}

#[test]
fn unrelated_nodes_do_not_remove_matched_reachability() {
    let before = snapshot(&["a", "b"], &[("a", "b")]);
    let after = snapshot(&["a", "b", "unrelated"], &[("a", "b")]);
    assert!(
        !delta::compare(&before, &after)
            .events
            .iter()
            .any(|event| event.kind == "reach_removed")
    );
}

#[test]
fn external_targets_are_boundary_evidence() {
    let mut state = snapshot(&["a"], &[("a", "external")]);
    state.relationships[0].resolution = ResolutionStatus::External;
    let result = graph::analyze_projection(&state, UnitKind::Module, Projection::Full);
    assert_eq!(result.pc.denominator, 1);
    assert_eq!(result.boundary.len(), 1);
    assert!(result.edges.is_empty());
}

#[test]
fn declaration_removal_with_equal_partial_coverage_is_real() {
    let mut before = snapshot(&["a", "b"], &[("a", "b"), ("b", "a")]);
    before.units[1].path = "a.rs".into();
    before.coverage.push(Coverage {
        path: "a.rs".into(),
        language: "rust".into(),
        status: CoverageStatus::Partial,
        effective: CoverageStatus::Partial,
        limitations: vec!["Dynamic targets are unresolved.".into()],
    });
    let mut after = snapshot(&["a"], &[]);
    after.coverage = before.coverage.clone();
    assert!(
        delta::compare(&before, &after)
            .events
            .iter()
            .any(|event| event.kind == "cycle_resolved")
    );
    after.coverage[0].status = CoverageStatus::Unavailable;
    assert!(
        !delta::compare(&before, &after)
            .events
            .iter()
            .any(|event| event.kind == "cycle_resolved")
    );
}

#[test]
fn duplicate_unit_identity_does_not_match_endpoints() {
    let mut before = snapshot(&["a", "b"], &[]);
    before.units.push(before.units[0].clone());
    let after = snapshot(&["a", "b"], &[("a", "b")]);
    assert!(
        !delta::compare(&before, &after)
            .events
            .iter()
            .any(|event| event.kind == "reach_added")
    );
}

#[test]
fn ambiguous_cycle_identity_stays_uncertain() {
    let mut before = snapshot(&["a", "b"], &[("a", "b"), ("b", "a")]);
    before.units.push(before.units[0].clone());
    let change = delta::compare(&before, &before);
    assert!(
        change
            .events
            .iter()
            .any(|event| event.kind == "match_uncertain")
    );
    assert!(
        !change
            .events
            .iter()
            .any(|event| event.kind.starts_with("cycle_"))
    );
}

#[test]
fn retained_resolved_support_prevents_false_coverage_loss() {
    let before = snapshot(&["a", "b"], &[("a", "b")]);
    let mut after = before.clone();
    let mut external = after.relationships[0].clone();
    external.target = "external".into();
    external.resolution = ResolutionStatus::External;
    after.relationships.push(external);
    assert!(
        !delta::compare(&before, &after)
            .events
            .iter()
            .any(|event| event.kind == "coverage_lost")
    );
}

#[test]
fn generic_resource_evidence_does_not_match_externalized_references() {
    let mut before = snapshot(&["a", "b"], &[("a", "b")]);
    before.relationships[0].kind = RelationshipKind::Resource;
    before.relationships[0].evidence[0].detail = "url".into();
    let mut after = before.clone();
    after.relationships[0].target = "https://example.test/asset".into();
    after.relationships[0].resolution = ResolutionStatus::External;
    assert!(
        !delta::compare(&before, &after)
            .events
            .iter()
            .any(|event| event.kind == "coverage_lost")
    );
}

#[test]
fn unchanged_source_file_rename_remains_an_uncertain_match() {
    let mut before = snapshot(&["old", "b"], &[("old", "b"), ("b", "old")]);
    before.units[1].source_hash = Some("other".into());
    let mut after = snapshot(&["new", "b"], &[("new", "b"), ("b", "new")]);
    after.units[1].source_hash = Some("other".into());
    before.units[0].source_hash = Some("same".into());
    after.units[0].source_hash = Some("same".into());
    after.removed_paths.push("old.rs".into());
    let change = delta::compare(&before, &after);
    assert!(
        change
            .events
            .iter()
            .any(|event| event.kind == "match_uncertain"
                && event.units.contains(&"old".into())
                && event.units.contains(&"new".into()))
    );
    assert!(
        !change
            .events
            .iter()
            .any(|event| event.kind == "cycle_resolved"
                || event.kind == "dependency_removed"
                || event.kind == "reach_removed")
    );
}

#[test]
fn equal_source_hashes_do_not_make_ambiguous_renames_certain() {
    let mut before = snapshot(
        &["first", "second"],
        &[("first", "second"), ("second", "first")],
    );
    let mut after = snapshot(&["third", "fourth"], &[]);
    for unit in before.units.iter_mut().chain(&mut after.units) {
        unit.source_hash = Some("same".into());
    }
    after.removed_paths = vec!["first.rs".into(), "second.rs".into()];
    let change = delta::compare(&before, &after);
    assert!(
        change
            .events
            .iter()
            .any(|event| event.kind == "match_uncertain" && event.units.len() == 4)
    );
    assert!(
        !change
            .events
            .iter()
            .any(|event| event.kind == "cycle_resolved")
    );
}

#[test]
fn rename_candidates_require_qualified_type_ownership() {
    let mut before = snapshot(&["old"], &[]);
    let mut after = snapshot(&["new"], &[]);
    before.units[0].kind = UnitKind::Type;
    before.units[0].name = "FirstOwner::Item".into();
    before.units[0].source_hash = Some("same".into());
    after.units[0].kind = UnitKind::Type;
    after.units[0].name = "SecondOwner::Item".into();
    after.units[0].source_hash = Some("same".into());
    assert!(
        !delta::compare(&before, &after)
            .events
            .iter()
            .any(|event| event.kind == "match_uncertain")
    );
}

#[test]
fn ordered_type_dependencies_report_source_order_changes() {
    let mut before = snapshot(&["a", "b", "c"], &[("a", "b"), ("a", "c")]);
    for unit in &mut before.units {
        unit.kind = UnitKind::Type;
    }
    for (index, edge) in before.relationships.iter_mut().enumerate() {
        edge.ordered = true;
        edge.kind = RelationshipKind::Inherit;
        edge.evidence[0].start = index * 10;
        edge.evidence[0].end = index * 10 + 1;
    }
    let mut after = before.clone();
    after.relationships[0].evidence[0].start = 10;
    after.relationships[0].evidence[0].end = 11;
    after.relationships[1].evidence[0].start = 0;
    after.relationships[1].evidence[0].end = 1;
    let change = delta::compare(&before, &after);
    let event = change
        .events
        .iter()
        .find(|event| event.kind == "relationship_order_changed")
        .unwrap();
    assert_eq!(event.before, ["b", "c"]);
    assert_eq!(event.after, ["c", "b"]);
    assert!(!event.changed.is_empty());
    assert_eq!(event.after_path.as_ref().unwrap().units, ["a", "c"]);
}

#[test]
fn ordered_support_offsets_and_duplicates_do_not_change_target_order() {
    let mut before = snapshot(&["a", "b", "c"], &[("a", "b"), ("a", "c")]);
    for unit in &mut before.units {
        unit.kind = UnitKind::Type;
    }
    for (index, edge) in before.relationships.iter_mut().enumerate() {
        edge.ordered = true;
        edge.evidence[0].start = index * 10;
        edge.evidence[0].end = index * 10 + 1;
    }
    let mut after = before.clone();
    for edge in &mut after.relationships {
        edge.evidence[0].start += 50;
        edge.evidence[0].end += 50;
        edge.evidence.push(edge.evidence[0].clone());
    }
    assert!(
        !delta::compare(&before, &after)
            .events
            .iter()
            .any(|event| event.kind == "relationship_order_changed")
    );
}
