//! Add cognitive-complexity points for functions in recursive call cycles.
//!
//! The shared program graph supplies only statically proven execution edges.
//! Symbol references retain their source identities while invocation evidence
//! can reach callable values, accessors, and constructors. Uncertain dispatch
//! and non-invoked values do not create cycles.

use std::collections::HashSet;

use petgraph::algo::kosaraju_scc;

use crate::graph::ExecutionGraph;
use crate::model::{ComplexityContribution, FileFacts};

/// Add one cognitive-complexity point to each function in a resolved cycle.
pub(crate) fn annotate(
    facts: &mut [FileFacts],
    graph: &ExecutionGraph,
    project: &crate::languages::csharp::project::Project,
) {
    let mut nodes = recursive(graph);
    if !project.views.is_empty()
        && nodes
            .iter()
            .any(|id| !project.owners(&facts[id.file_index].target).is_empty())
    {
        let configured = nodes
            .iter()
            .copied()
            .filter(|id| !project.owners(&facts[id.file_index].target).is_empty())
            .collect::<HashSet<_>>();
        nodes.retain(|id| !configured.contains(id));
        for reachable in &project.reachable {
            let active = |index: usize, position: usize| {
                let path = &facts[index].target;
                project
                    .owners(path)
                    .iter()
                    .any(|view| reachable.contains(view) && project.active(path, *view, position))
            };
            let view = graph.filter_map(
                |_, id| configured.contains(id).then_some(*id),
                |_, edge| {
                    (active(edge.caller_file, edge.call_start_byte)
                        && active(edge.callee_file, edge.callee_start_byte))
                    .then_some(*edge)
                },
            );
            nodes.extend(recursive(&view));
        }
    }
    for function_id in nodes {
        let fact = &mut facts[function_id.file_index];
        let function = &mut fact.analysis.functions[function_id.function_index];
        let contribution = ComplexityContribution {
            kind: "recursion".into(),
            value: 1,
            start_byte: function.start_byte,
            end_byte: function.start_byte,
            line: memchr::memchr_iter(b'\n', &fact.source[..function.start_byte]).count() + 1,
        };
        function.cognitive_complexity += 1;
        function.cognitive_contributions.push(contribution);
        function
            .cognitive_contributions
            .sort_by_key(|item| (item.start_byte, item.end_byte, item.kind.clone()));
    }
}

fn recursive(graph: &ExecutionGraph) -> HashSet<crate::graph::ProgramFunctionId> {
    kosaraju_scc(graph)
        .into_iter()
        .filter(|component| {
            component.len() > 1
                || component
                    .first()
                    .is_some_and(|node| graph.find_edge(*node, *node).is_some())
        })
        .flatten()
        .map(|node| graph[node])
        .collect()
}
