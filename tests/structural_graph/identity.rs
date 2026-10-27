use super::*;

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
