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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        ComplexityContribution, FileAnalysis, ModulePath, Reference, ReferenceAnalysis,
        ReferenceKind, SymbolId,
    };
    use std::path::PathBuf;
    use std::sync::Arc;

    fn file(path: &str, functions: &[(&str, usize, usize)]) -> FileFacts {
        let functions = functions
            .iter()
            .map(|(name, start, end)| crate::model::FunctionAnalysis {
                name: (*name).into(),
                start_byte: *start,
                end_byte: *end,
                nloc: 1,
                cyclomatic_complexity: 1,
                cyclomatic_density: 1.0,
                contributions: vec![ComplexityContribution {
                    kind: "baseline".into(),
                    value: 1,
                    start_byte: *start,
                    end_byte: *start,
                    line: 1,
                }],
                cognitive_complexity: 0,
                cognitive_contributions: Vec::new(),
            })
            .collect();
        FileFacts {
            source: Arc::<[u8]>::from(Vec::new()),
            target: PathBuf::from(path),
            aliases: vec![PathBuf::from(path)],
            path: PathBuf::from(path),
            module: ModulePath::default(),
            definitions: Vec::new(),
            imports: Vec::new(),
            references: Vec::new(),
            locals: Vec::new(),
            analysis: FileAnalysis {
                path: path.into(),
                hash: String::new(),
                language: "rust".into(),
                nloc: 0,
                functions,
                injections: Vec::new(),
                resolution: Vec::new(),
            },
        }
    }

    fn call(fact: &mut FileFacts, owner: usize, target_file: &str, target: usize) {
        fact.references.push(Reference {
            path: vec![format!("f{target}")],
            module: ModulePath::default(),
            kind: ReferenceKind::Call,
            start_byte: owner + 1,
            end_byte: owner + 2,
            scope_start: owner,
            scope_end: owner + 10,
            call_owner: Some(owner),
        });
        fact.analysis.resolution.push(ReferenceAnalysis {
            path: vec![format!("f{target}")],
            start_byte: owner + 1,
            end_byte: owner + 2,
            resolution: Resolution::Exact(SymbolId {
                file: target_file.into(),
                module: ModulePath::default(),
                name: format!("f{target}"),
                kind: DefinitionKind::Function,
                start_byte: target,
            }),
        });
    }

    fn assert_recursion(facts: &mut [FileFacts], expected: &[(&str, usize)]) {
        annotate(facts);
        let actual = facts
            .iter()
            .flat_map(|fact| {
                fact.analysis
                    .functions
                    .iter()
                    .filter(|function| {
                        function
                            .cognitive_contributions
                            .iter()
                            .any(|contribution| contribution.kind == "recursion")
                    })
                    .map(|function| (fact.analysis.path.as_str(), function.start_byte))
            })
            .collect::<HashSet<_>>();
        assert_eq!(actual, expected.iter().copied().collect::<HashSet<_>>());
    }

    #[test]
    fn marks_direct_and_mutual_function_cycles_once() {
        let mut fact = file(
            "a.rs",
            &[
                ("self_recursive", 0, 10),
                ("left", 20, 30),
                ("right", 40, 50),
            ],
        );
        call(&mut fact, 0, "a.rs", 0);
        call(&mut fact, 20, "a.rs", 40);
        call(&mut fact, 40, "a.rs", 20);
        assert_recursion(&mut [fact], &[("a.rs", 0), ("a.rs", 20), ("a.rs", 40)]);
    }

    #[test]
    fn excludes_nonexact_and_nonfunction_edges() {
        let mut fact = file("a.rs", &[("recursive", 0, 10), ("other", 20, 30)]);
        call(&mut fact, 0, "a.rs", 0);
        call(&mut fact, 0, "a.rs", 20);
        let last = fact.analysis.resolution.last_mut().unwrap();
        last.resolution = Resolution::Ambiguous(Vec::new());
        call(&mut fact, 20, "a.rs", 0);
        let last = fact.analysis.resolution.last_mut().unwrap();
        if let Resolution::Exact(target) = &mut last.resolution {
            target.kind = DefinitionKind::Method;
        }
        assert_recursion(&mut [fact], &[("a.rs", 0)]);
    }

    #[test]
    fn maps_cross_file_edges_by_report_path_and_start() {
        let mut caller = file("caller.rs", &[("caller", 0, 10)]);
        let mut callee = file("callee.rs", &[("callee", 100, 110)]);
        call(&mut caller, 0, "callee.rs", 100);
        call(&mut callee, 100, "caller.rs", 0);
        assert_recursion(
            &mut [caller, callee],
            &[("caller.rs", 0), ("callee.rs", 100)],
        );
    }
}
