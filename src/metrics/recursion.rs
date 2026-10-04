//! Add cognitive-complexity points for functions in recursive call cycles.
//!
//! Only direct call sites with an exact function resolution become graph
//! edges. Ambiguous references, function values, methods, and names in
//! deferred closures are left on the normal reference-reporting path.

use std::collections::{BTreeMap, HashMap, HashSet};

use petgraph::algo::kosaraju_scc;
use petgraph::graph::{DiGraph, NodeIndex};

use crate::model::{DefinitionKind, FileFacts, Resolution};

/// Add one cognitive-complexity point to each function in a resolved cycle.
pub(crate) fn annotate(facts: &mut [FileFacts]) {
    let mut graph = DiGraph::<(usize, usize), ()>::new();
    let mut function_nodes = BTreeMap::<(usize, usize), NodeIndex>::new();
    let mut files_by_path = HashMap::<String, usize>::new();

    for (file_index, fact) in facts.iter().enumerate() {
        files_by_path.insert(fact.analysis.path.clone(), file_index);
        for (function_index, function) in fact.analysis.functions.iter().enumerate() {
            let key = (file_index, function.start_byte);
            function_nodes
                .entry(key)
                .or_insert_with(|| graph.add_node((file_index, function_index)));
        }
    }

    for (file_index, fact) in facts.iter().enumerate() {
        for (reference, analysis) in fact.references.iter().zip(&fact.analysis.resolution) {
            let Some(call_owner) = reference.call_owner else {
                continue;
            };
            let Resolution::Exact(target) = &analysis.resolution else {
                continue;
            };
            if target.kind != DefinitionKind::Function {
                continue;
            }
            let Some(&target_file) = files_by_path.get(&target.file) else {
                continue;
            };
            let caller = function_nodes.get(&(file_index, call_owner));
            let callee = function_nodes.get(&(target_file, target.start_byte));
            if let (Some(&caller), Some(&callee)) = (caller, callee) {
                graph.add_edge(caller, callee, ());
            }
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
        let function = &mut facts[file_index].analysis.functions[function_index];
        let Some(mut contribution) = function
            .contributions
            .iter()
            .find(|contribution| contribution.kind == "baseline")
            .cloned()
        else {
            continue;
        };
        contribution.kind = "recursion".into();
        contribution.value = 1;
        function.cognitive_complexity += 1;
        function.cognitive_contributions.push(contribution);
        function
            .cognitive_contributions
            .sort_by_key(|item| (item.start_byte, item.end_byte, item.kind.clone()));
    }
}
