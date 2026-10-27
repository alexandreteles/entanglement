use super::*;

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
