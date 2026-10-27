use super::*;

pub(super) fn eligible(
    declaration: &EffectiveDeclaration,
    records: &BTreeMap<String, TypeRecord<'_>>,
    snapshot: &StructureSnapshot,
) -> bool {
    let Some(record) = records.get(&declaration.unit) else {
        return false;
    };
    declaration.status != CoverageStatus::Unavailable
        && declaration.conflicts.is_empty()
        && !declaration.obligations.iter().any(|obligation| {
            obligation.starts_with("unresolved:") || obligation.starts_with("promotion_ambiguous")
        })
        && !record.record.types.iter().any(|expression| {
            expression.form.contains("type_parameter")
                || expression.text.contains('~')
                || expression.text.contains('|')
        })
        && constraints_known(record, records, snapshot)
        && snapshot
            .relationships
            .iter()
            .filter(|edge| {
                edge.source == declaration.unit && edge.kind == RelationshipKind::Compose
            })
            .all(|edge| edge.resolution == ResolutionStatus::Resolved)
}

fn constraints_known(
    record: &TypeRecord<'_>,
    records: &BTreeMap<String, TypeRecord<'_>>,
    snapshot: &StructureSnapshot,
) -> bool {
    let mut visited = BTreeSet::new();
    let mut pending = vec![extract::type_id(record.fact, record.store, record.record)];
    while let Some(unit) = pending.pop() {
        if !visited.insert(unit.clone()) {
            continue;
        }
        let Some(record) = records.get(&unit) else {
            return false;
        };
        if record.store.semantic.structure_errors
            || record.record.types.iter().any(|expression| {
                expression.form.contains("type_parameter")
                    || expression.text.contains('~')
                    || expression.text.contains('|')
            })
        {
            return false;
        }
        if !direct_constraints_known(record, records, snapshot) {
            return false;
        }
        pending.extend(
            snapshot
                .relationships
                .iter()
                .filter(|edge| {
                    edge.source == unit
                        && edge.kind == RelationshipKind::Compose
                        && edge.resolution == ResolutionStatus::Resolved
                })
                .map(|edge| edge.target.clone()),
        );
    }
    true
}

fn direct_constraints_known(
    record: &TypeRecord<'_>,
    records: &BTreeMap<String, TypeRecord<'_>>,
    snapshot: &StructureSnapshot,
) -> bool {
    record
        .record
        .types
        .iter()
        .filter(|expression| expression.form == "type_elem")
        .all(|expression| {
            snapshot
                .relationships
                .iter()
                .filter(|edge| {
                    edge.source == extract::type_id(record.fact, record.store, record.record)
                        && edge.kind == RelationshipKind::Compose
                        && edge.resolution == ResolutionStatus::Resolved
                })
                .filter_map(|edge| records.get(&edge.target))
                .any(|target| {
                    target.record.kind == "interface" && target.record.name == expression.text
                })
        })
}
