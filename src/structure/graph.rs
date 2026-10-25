use super::model::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Analyze the two projections with separate module and type universes.
pub fn analyze(snapshot: &StructureSnapshot) -> Vec<GraphAnalysis> {
    [UnitKind::Module, UnitKind::Type]
        .into_iter()
        .flat_map(|kind| {
            [Projection::Contract, Projection::Full]
                .into_iter()
                .map(move |projection| analyze_projection(snapshot, kind, projection))
        })
        .collect()
}

/// Calculate exact internal reachability, including each unit itself.
///
/// External and unresolved targets remain in boundary evidence. Self-edges do
/// not establish architectural cycles. Containment is not a dependency.
pub fn analyze_projection(
    snapshot: &StructureSnapshot,
    granularity: UnitKind,
    projection: Projection,
) -> GraphAnalysis {
    let units: Vec<String> = snapshot
        .units
        .iter()
        .filter(|unit| unit.kind == granularity)
        .map(|unit| unit.id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let selected: BTreeSet<&str> = units.iter().map(String::as_str).collect();
    let (edges, boundary) = project_edges(snapshot, &selected, projection);
    let adjacency = adjacency(&units, &edges);
    let components = components(&units, &adjacency);
    let membership: BTreeMap<&str, usize> = components
        .iter()
        .enumerate()
        .flat_map(|(index, members)| members.iter().map(move |member| (member.as_str(), index)))
        .collect();
    let mut component_edges = vec![BTreeSet::new(); components.len()];
    for edge in &edges {
        let source = membership[edge.source.as_str()];
        let target = membership[edge.target.as_str()];
        if source != target {
            component_edges[source].insert(target);
        }
    }
    let numerator = component_edges
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let mut reached = BTreeSet::from([index]);
            let mut pending = vec![index];
            while let Some(current) = pending.pop() {
                for target in &component_edges[current] {
                    if reached.insert(*target) {
                        pending.push(*target);
                    }
                }
            }
            components[index].len() as u64
                * reached
                    .iter()
                    .map(|target| components[*target].len() as u64)
                    .sum::<u64>()
        })
        .sum();
    let denominator = (units.len() as u64).pow(2);
    let cycles = components
        .iter()
        .filter(|members| members.len() > 1)
        .map(|members| {
            let source = &members[0];
            let target = adjacency[source]
                .iter()
                .find(|target| members.contains(target))
                .unwrap();
            let mut witness = find_path(target, source, &adjacency, &edges).unwrap();
            witness.units.insert(0, source.clone());
            witness
                .evidence
                .extend(edge_evidence(source, target, &edges));
            witness.evidence.sort();
            witness.evidence.dedup();
            Cycle {
                members: members.clone(),
                witness,
            }
        })
        .collect();
    let mut impacts: Vec<Impact> = units
        .iter()
        .enumerate()
        .map(|(index, _)| Impact {
            unit: index,
            dependencies: Vec::new(),
            dependents: Vec::new(),
            predecessors: Vec::new(),
        })
        .collect();
    let indices: BTreeMap<&str, usize> = units
        .iter()
        .enumerate()
        .map(|(index, unit)| (unit.as_str(), index))
        .collect();
    for source in &units {
        let predecessors = predecessors(source, &adjacency);
        for target in predecessors.keys().filter(|target| *target != source) {
            impacts[indices[source.as_str()]]
                .dependencies
                .push(indices[target.as_str()]);
            impacts[indices[target.as_str()]]
                .dependents
                .push(indices[source.as_str()]);
        }
        impacts[indices[source.as_str()]].predecessors = predecessors
            .iter()
            .map(|(target, parent)| [indices[target.as_str()], indices[parent.as_str()]])
            .collect();
    }
    GraphAnalysis {
        granularity,
        projection,
        units,
        edges,
        boundary,
        cycles,
        pc: PropagationCost {
            value: (denominator > 0).then(|| numerator as f64 / denominator as f64),
            numerator,
            denominator,
        },
        impacts,
    }
}

fn project_edges(
    snapshot: &StructureSnapshot,
    selected: &BTreeSet<&str>,
    projection: Projection,
) -> (Vec<Relationship>, Vec<Relationship>) {
    let mut grouped: BTreeMap<(&str, &str, RelationshipKind, ResolutionStatus), Relationship> =
        BTreeMap::new();
    for edge in snapshot.relationships.iter().filter(|edge| {
        selected.contains(edge.source.as_str()) && (projection == Projection::Full || edge.contract)
    }) {
        if edge.source == edge.target {
            continue;
        }
        let key = (
            edge.source.as_str(),
            edge.target.as_str(),
            edge.kind,
            edge.resolution,
        );
        let entry = grouped.entry(key).or_insert_with(|| {
            let mut edge = edge.clone();
            edge.evidence.clear();
            edge
        });
        entry.contract |= edge.contract;
        entry.ordered |= edge.ordered;
        entry.evidence.extend(edge.evidence.clone());
    }
    let mut internal = Vec::new();
    let mut boundary = Vec::new();
    for mut edge in grouped.into_values() {
        edge.evidence.sort();
        edge.evidence.dedup();
        if edge.resolution == ResolutionStatus::Resolved && selected.contains(edge.target.as_str())
        {
            internal.push(edge);
        } else {
            boundary.push(edge);
        }
    }
    (internal, boundary)
}

fn adjacency(units: &[String], edges: &[Relationship]) -> BTreeMap<String, BTreeSet<String>> {
    let mut result: BTreeMap<String, BTreeSet<String>> = units
        .iter()
        .map(|unit| (unit.clone(), BTreeSet::new()))
        .collect();
    for edge in edges {
        result
            .get_mut(&edge.source)
            .unwrap()
            .insert(edge.target.clone());
    }
    result
}

/// Reuse the graph library's iterative SCC traversal.
fn components(
    units: &[String],
    adjacency: &BTreeMap<String, BTreeSet<String>>,
) -> Vec<Vec<String>> {
    let mut graph = petgraph::graph::DiGraph::<&str, ()>::new();
    let nodes: BTreeMap<_, _> = units
        .iter()
        .map(|unit| (unit.as_str(), graph.add_node(unit)))
        .collect();
    for (source, targets) in adjacency {
        for target in targets {
            graph.add_edge(nodes[source.as_str()], nodes[target.as_str()], ());
        }
    }
    let mut components: Vec<Vec<String>> = petgraph::algo::kosaraju_scc(&graph)
        .into_iter()
        .map(|members| {
            let mut members: Vec<_> = members
                .into_iter()
                .map(|index| graph[index].to_owned())
                .collect();
            members.sort();
            members
        })
        .collect();
    components.sort();
    components
}

fn edge_evidence(source: &str, target: &str, edges: &[Relationship]) -> Vec<Evidence> {
    edges
        .iter()
        .filter(|edge| edge.source == source && edge.target == target)
        .flat_map(|edge| edge.evidence.clone())
        .collect()
}

fn predecessors(
    source: &str,
    adjacency: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeMap<String, String> {
    let mut previous = BTreeMap::from([(source.to_owned(), source.to_owned())]);
    let mut pending = VecDeque::from([source.to_owned()]);
    while let Some(current) = pending.pop_front() {
        for target in &adjacency[&current] {
            if previous.contains_key(target) {
                continue;
            }
            previous.insert(target.clone(), current.clone());
            pending.push_back(target.clone());
        }
    }
    previous
}

fn witness_from(
    source: &str,
    target: &str,
    previous: &BTreeMap<String, String>,
    edges: &[Relationship],
) -> Option<Witness> {
    if !previous.contains_key(target) {
        return None;
    }
    let mut units = vec![target.to_owned()];
    while units.last().unwrap() != source {
        units.push(previous[units.last().unwrap()].clone());
    }
    units.reverse();
    let mut evidence: Vec<_> = units
        .windows(2)
        .flat_map(|pair| edge_evidence(&pair[0], &pair[1], edges))
        .collect();
    evidence.sort();
    evidence.dedup();
    Some(Witness { units, evidence })
}

/// Rebuild a shortest dependency path from its compact predecessor tree.
/// Return no path when either endpoint is absent or unreachable.
pub fn witness(graph: &GraphAnalysis, source: &str, target: &str) -> Option<Witness> {
    let source = graph
        .units
        .binary_search_by(|unit| unit.as_str().cmp(source))
        .ok()?;
    let target = graph
        .units
        .binary_search_by(|unit| unit.as_str().cmp(target))
        .ok()?;
    let impact = graph.impacts.iter().find(|impact| impact.unit == source)?;
    let mut indices = vec![target];
    while *indices.last()? != source {
        let index = impact
            .predecessors
            .binary_search_by_key(indices.last()?, |pair| pair[0])
            .ok()?;
        indices.push(impact.predecessors[index][1]);
    }
    indices.reverse();
    let units: Vec<_> = indices
        .into_iter()
        .map(|index| graph.units[index].clone())
        .collect();
    let mut evidence: Vec<_> = units
        .windows(2)
        .flat_map(|pair| edge_evidence(&pair[0], &pair[1], &graph.edges))
        .collect();
    evidence.sort();
    evidence.dedup();
    Some(Witness { units, evidence })
}

fn find_path(
    source: &str,
    target: &str,
    adjacency: &BTreeMap<String, BTreeSet<String>>,
    edges: &[Relationship],
) -> Option<Witness> {
    witness_from(source, target, &predecessors(source, adjacency), edges)
}
