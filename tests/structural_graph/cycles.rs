use super::*;

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
