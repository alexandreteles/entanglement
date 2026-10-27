use super::*;

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
