//! Add cognitive-complexity points for functions in recursive call cycles.
//!
//! The shared program graph supplies only statically proven execution edges.
//! Symbol references retain their source identities while invocation evidence
//! can reach callable values, accessors, and constructors. Uncertain dispatch
//! and non-invoked values do not create cycles.

use std::collections::{BTreeMap, HashSet};

use petgraph::algo::kosaraju_scc;
use petgraph::graph::{DiGraph, NodeIndex};

use crate::graph::CallEdge;
use crate::model::{ComplexityContribution, FileFacts};

/// Add one cognitive-complexity point to each function in a resolved cycle.
pub(crate) fn annotate(facts: &mut [FileFacts], calls: &[CallEdge]) {
    let mut graph = DiGraph::<(usize, usize), ()>::new();
    let mut function_nodes = BTreeMap::<(usize, usize), NodeIndex>::new();

    for (file_index, fact) in facts.iter().enumerate() {
        for (function_index, function) in fact.analysis.functions.iter().enumerate() {
            let key = (file_index, function.start_byte);
            function_nodes
                .entry(key)
                .or_insert_with(|| graph.add_node((file_index, function_index)));
        }
    }

    for edge in calls {
        let caller = function_nodes.get(&(edge.caller_file, edge.caller_start_byte));
        let callee = function_nodes.get(&(edge.callee_file, edge.callee_start_byte));
        if let (Some(&caller), Some(&callee)) = (caller, callee) {
            graph.add_edge(caller, callee, ());
        }
    }

    let recursive_nodes: HashSet<_> = kosaraju_scc(&graph)
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
        let (file_index, function_index) = graph[node];
        let fact = &mut facts[file_index];
        let function = &mut fact.analysis.functions[function_index];
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
