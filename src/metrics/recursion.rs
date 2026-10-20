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
pub(crate) fn annotate(facts: &mut [FileFacts], graph: &ExecutionGraph) {
    let recursive_nodes: HashSet<_> = kosaraju_scc(graph)
        .into_iter()
        .filter(|component| {
            component.len() > 1
                || component
                    .first()
                    .is_some_and(|node| graph.find_edge(*node, *node).is_some())
        })
        .flatten()
        .collect();

    for node in recursive_nodes {
        let function_id = graph[node];
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
