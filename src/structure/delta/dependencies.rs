use super::*;

pub(super) fn compare_dependencies(
    current: &GraphAnalysis,
    coverage_lost: bool,
    changed: &[Relationship],
    events: &mut Vec<StructuralEvent>,
) {
    for edge in changed {
        let added = current
            .edges
            .iter()
            .any(|candidate| edge_key(candidate) == edge_key(edge));
        if !added && coverage_lost {
            continue;
        }
        let mut finding = event(
            if added {
                "dependency_added"
            } else {
                "dependency_removed"
            },
            current,
            vec![edge.source.clone(), edge.target.clone()],
            std::slice::from_ref(edge),
        );
        if added {
            finding.after = vec![format!("{:?}", edge.kind)];
        } else {
            finding.before = vec![format!("{:?}", edge.kind)];
        }
        events.push(finding);
    }
}

pub(super) fn compare_reachability(
    previous: &GraphAnalysis,
    current: &GraphAnalysis,
    matched: &BTreeSet<String>,
    coverage_lost: bool,
    changed: &[Relationship],
    events: &mut Vec<StructuralEvent>,
) {
    let previous_paths = reachable_pairs(previous);
    let current_paths = reachable_pairs(current);
    for source in matched {
        for target in matched.iter().filter(|target| *target != source) {
            let key = (source.clone(), target.clone());
            let old_reached = previous_paths.contains(&key);
            let new_reached = current_paths.contains(&key);
            if old_reached == new_reached || (!new_reached && coverage_lost) {
                continue;
            }
            let old = graph::witness(previous, source, target);
            let new = graph::witness(current, source, target);
            let mut finding = event(
                if new.is_some() {
                    "reach_added"
                } else {
                    "reach_removed"
                },
                current,
                vec![source.clone(), target.clone()],
                &path_changes(old.as_ref(), new.as_ref(), changed),
            );
            finding.before_path = old;
            finding.after_path = new;
            events.push(finding);
        }
    }
}

fn edge_key(edge: &Relationship) -> (&str, &str, RelationshipKind) {
    (&edge.source, &edge.target, edge.kind)
}

pub(super) fn changed_edges(before: &[Relationship], after: &[Relationship]) -> Vec<Relationship> {
    before
        .iter()
        .filter(|edge| {
            !after
                .iter()
                .any(|candidate| edge_key(edge) == edge_key(candidate))
        })
        .chain(after.iter().filter(|edge| {
            !before
                .iter()
                .any(|candidate| edge_key(edge) == edge_key(candidate))
        }))
        .cloned()
        .collect()
}

fn reachable_pairs(graph: &GraphAnalysis) -> BTreeSet<(String, String)> {
    graph
        .impacts
        .iter()
        .flat_map(|impact| {
            impact.dependencies.iter().map(|target| {
                (
                    graph.units[impact.unit].clone(),
                    graph.units[*target].clone(),
                )
            })
        })
        .collect()
}

fn path_changes(
    before: Option<&Witness>,
    after: Option<&Witness>,
    changed: &[Relationship],
) -> Vec<Relationship> {
    changed
        .iter()
        .filter(|edge| {
            before.into_iter().chain(after).any(|path| {
                path.units
                    .windows(2)
                    .any(|pair| pair[0] == edge.source && pair[1] == edge.target)
            })
        })
        .cloned()
        .collect()
}
