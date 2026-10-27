use super::*;

fn ordered_targets(graph: &GraphAnalysis) -> BTreeMap<String, Vec<String>> {
    let mut supports = BTreeMap::<String, BTreeSet<(usize, usize, String)>>::new();
    for edge in graph.edges.iter().filter(|edge| edge.ordered) {
        supports.entry(edge.source.clone()).or_default().extend(
            edge.evidence
                .iter()
                .filter(|evidence| !matches!(evidence.detail.as_str(), "relation" | "embed_ptr"))
                .map(|evidence| (evidence.start, evidence.end, edge.target.clone())),
        );
    }
    supports
        .into_iter()
        .map(|(source, targets)| {
            (
                source,
                targets.into_iter().map(|(_, _, target)| target).collect(),
            )
        })
        .collect()
}

pub(super) fn compare_order(
    previous: &GraphAnalysis,
    current: &GraphAnalysis,
    matched: &BTreeSet<String>,
    events: &mut Vec<StructuralEvent>,
) {
    let previous_order = ordered_targets(previous);
    let current_order = ordered_targets(current);
    for (source, before) in previous_order
        .iter()
        .filter(|(source, _)| matched.contains(*source))
    {
        let Some(after) = current_order.get(source).filter(|after| *after != before) else {
            continue;
        };
        let mut previous_targets = before.clone();
        previous_targets.sort();
        let mut current_targets = after.clone();
        current_targets.sort();
        if previous_targets != current_targets {
            continue;
        }
        let changed: Vec<_> = previous
            .edges
            .iter()
            .chain(&current.edges)
            .filter(|edge| edge.ordered && edge.source == *source)
            .cloned()
            .collect();
        let mut finding = event(
            "relationship_order_changed",
            current,
            vec![source.clone()],
            &changed,
        );
        finding.before = before.clone();
        finding.after = after.clone();
        finding.before_paths = before
            .iter()
            .filter_map(|target| graph::witness(previous, source, target))
            .collect();
        finding.after_paths = after
            .iter()
            .filter_map(|target| graph::witness(current, source, target))
            .collect();
        finding.before_path = finding.before_paths.first().cloned();
        finding.after_path = finding.after_paths.first().cloned();
        events.push(finding);
    }
}
