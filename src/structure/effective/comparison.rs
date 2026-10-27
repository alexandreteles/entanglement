use super::comparison_values::{effective_values, semantic_signature};
use super::*;

pub(crate) fn compare(
    before: &[EffectiveDeclaration],
    after: &[EffectiveDeclaration],
) -> Vec<StructuralEvent> {
    let before = before
        .iter()
        .map(|declaration| (&declaration.unit, declaration))
        .collect::<BTreeMap<_, _>>();
    let current_units = after
        .iter()
        .map(|declaration| &declaration.unit)
        .collect::<BTreeSet<_>>();
    let mut events: Vec<_> = after
        .iter()
        .filter_map(|current| {
            let previous = before.get(&current.unit)?;
            if previous.status == CoverageStatus::Unavailable
                || current.status == CoverageStatus::Unavailable
            {
                return (previous.status != current.status).then(|| {
                    effective_event(
                        "effective_coverage",
                        previous,
                        current,
                        vec![format!("{:?}", previous.status).to_lowercase()],
                        vec![format!("{:?}", current.status).to_lowercase()],
                    )
                });
            }
            (semantic_signature(previous) != semantic_signature(current)).then(|| {
                effective_event(
                    "effective",
                    previous,
                    current,
                    effective_values(previous),
                    effective_values(current),
                )
            })
        })
        .collect();
    events.extend(
        after
            .iter()
            .filter(|declaration| !before.contains_key(&declaration.unit))
            .map(|declaration| presence_event("effective_added", declaration)),
    );
    events.extend(
        before
            .values()
            .filter(|declaration| !current_units.contains(&declaration.unit))
            .map(|declaration| presence_event("effective_removed", declaration)),
    );
    events.sort_by(|left, right| (&left.units, &left.kind).cmp(&(&right.units, &right.kind)));
    events
}

fn presence_event(kind: &str, declaration: &EffectiveDeclaration) -> StructuralEvent {
    let event_kind = if declaration.status == CoverageStatus::Unavailable {
        "effective_coverage"
    } else {
        kind
    };
    let values = if declaration.status == CoverageStatus::Unavailable {
        vec!["unavailable".to_owned()]
    } else {
        effective_values(declaration)
    };
    let mut event = effective_event(event_kind, declaration, declaration, values.clone(), values);
    if kind == "effective_added" {
        event.before.clear();
        event.before_path = None;
        event.before_paths.clear();
    } else {
        event.after.clear();
        event.after_path = None;
        event.after_paths.clear();
    }
    event
}

fn effective_event(
    kind: &str,
    previous: &EffectiveDeclaration,
    current: &EffectiveDeclaration,
    before: Vec<String>,
    after: Vec<String>,
) -> StructuralEvent {
    let before_paths = event_paths(previous);
    let after_paths = event_paths(current);
    StructuralEvent {
        kind: kind.to_owned(),
        granularity: UnitKind::Type,
        projection: Projection::Contract,
        units: vec![current.unit.clone()],
        before,
        after,
        changed: Vec::new(),
        before_path: before_paths.first().cloned(),
        after_path: after_paths.first().cloned(),
        before_paths,
        after_paths,
    }
}

fn event_paths(declaration: &EffectiveDeclaration) -> Vec<Witness> {
    let mut paths: Vec<_> = declaration
        .members
        .iter()
        .map(|member| {
            let mut evidence = member.evidence.clone();
            evidence.extend(declaration.evidence.clone());
            evidence.sort();
            evidence.dedup();
            Witness {
                units: member.provenance.clone(),
                evidence,
            }
        })
        .collect();
    if paths.is_empty() {
        paths.push(Witness {
            units: vec![declaration.unit.clone()],
            evidence: declaration.evidence.clone(),
        });
    }
    paths
        .sort_by(|left, right| (&left.units, &left.evidence).cmp(&(&right.units, &right.evidence)));
    paths.dedup();
    paths
}
