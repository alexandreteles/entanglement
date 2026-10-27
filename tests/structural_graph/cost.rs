use super::*;

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
fn external_targets_are_boundary_evidence() {
    let mut state = snapshot(&["a"], &[("a", "external")]);
    state.relationships[0].resolution = ResolutionStatus::External;
    let result = graph::analyze_projection(&state, UnitKind::Module, Projection::Full);
    assert_eq!(result.pc.denominator, 1);
    assert_eq!(result.boundary.len(), 1);
    assert!(result.edges.is_empty());
}
