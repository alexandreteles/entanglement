use super::*;

pub(super) fn compare_cycles(
    previous: &GraphAnalysis,
    current: &GraphAnalysis,
    coverage_lost: bool,
    changed: &[Relationship],
    uncertain: &BTreeSet<String>,
    events: &mut Vec<StructuralEvent>,
) {
    for cycle in current
        .cycles
        .iter()
        .filter(|cycle| cycle.members.iter().all(|unit| !uncertain.contains(unit)))
    {
        let overlaps: Vec<_> = previous
            .cycles
            .iter()
            .filter(|previous| {
                previous
                    .members
                    .iter()
                    .any(|unit| cycle.members.contains(unit))
            })
            .collect();
        let kind = match overlaps.as_slice() {
            [] => "cycle_introduced",
            [previous] if previous.members == cycle.members => "cycle_persistent",
            [previous]
                if previous
                    .members
                    .iter()
                    .all(|unit| cycle.members.contains(unit)) =>
            {
                "cycle_expanded"
            }
            [_] => "cycle_changed",
            _ => "cycle_merged",
        };
        if coverage_lost && kind == "cycle_changed" {
            continue;
        }
        let affected: BTreeSet<_> = cycle
            .members
            .iter()
            .chain(overlaps.iter().flat_map(|cycle| cycle.members.iter()))
            .cloned()
            .collect();
        let relevant: Vec<_> = changed
            .iter()
            .filter(|edge| affected.contains(&edge.source) || affected.contains(&edge.target))
            .cloned()
            .collect();
        let mut finding = event(kind, current, affected.into_iter().collect(), &relevant);
        finding.before = overlaps
            .iter()
            .flat_map(|cycle| cycle.members.clone())
            .collect();
        finding.before.sort();
        finding.before.dedup();
        finding.after = cycle.members.clone();
        finding.before_path = overlaps.first().map(|cycle| cycle.witness.clone());
        finding.after_path = Some(cycle.witness.clone());
        events.push(finding);
    }
    if coverage_lost {
        return;
    }
    compare_cycle_removals(previous, current, changed, uncertain, events);
}

fn compare_cycle_removals(
    previous: &GraphAnalysis,
    current: &GraphAnalysis,
    changed: &[Relationship],
    uncertain: &BTreeSet<String>,
    events: &mut Vec<StructuralEvent>,
) {
    for cycle in previous
        .cycles
        .iter()
        .filter(|cycle| cycle.members.iter().all(|unit| !uncertain.contains(unit)))
    {
        let overlaps: Vec<_> = current
            .cycles
            .iter()
            .filter(|current| {
                current
                    .members
                    .iter()
                    .any(|unit| cycle.members.contains(unit))
            })
            .collect();
        let kind = match overlaps.len() {
            0 => "cycle_resolved",
            2.. => "cycle_split",
            _ => continue,
        };
        let mut finding = event(kind, current, cycle.members.clone(), changed);
        finding.before = cycle.members.clone();
        finding.after = overlaps
            .iter()
            .flat_map(|cycle| cycle.members.clone())
            .collect();
        finding.after.sort();
        finding.after.dedup();
        finding.before_path = Some(cycle.witness.clone());
        finding.after_path = overlaps.first().map(|cycle| cycle.witness.clone());
        events.push(finding);
    }
}
