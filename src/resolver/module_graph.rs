use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use crate::graph::ProgramGraph;
use crate::languages::ResolutionFamily;
use crate::model::DefinitionKind;

use super::{CrateKey, FileContext, joined_path};

type ModuleRoute = (usize, FileContext, PathBuf, BTreeSet<PathBuf>);

pub(super) fn file_contexts(graph: &ProgramGraph<'_>) -> Vec<Vec<FileContext>> {
    let by_path = graph
        .aliases()
        .iter()
        .flat_map(|(path, indexes)| {
            indexes
                .iter()
                .filter(|index| {
                    graph.file(**index).resolution_family == ResolutionFamily::RustCrates
                })
                .map(move |index| (path.clone(), *index))
        })
        .collect::<BTreeMap<_, _>>();
    let mut roots = graph
        .files()
        .iter()
        .enumerate()
        .filter(|(_, fact)| fact.resolution_family == ResolutionFamily::RustCrates)
        .flat_map(|(index, fact)| {
            fact.aliases
                .iter()
                .filter(|path| is_crate_root(path))
                .map(move |path| (path.clone(), index))
        })
        .collect::<Vec<_>>();
    roots.sort_by(|left, right| left.0.cmp(&right.0));

    let mut assignments = vec![BTreeSet::new(); graph.file_count()];
    let mut pending = VecDeque::new();
    for (root_path, index) in roots {
        pending.push_back((
            index,
            FileContext {
                crate_root: CrateKey::Logical(root_path.clone()),
                module: Vec::new(),
            },
            root_path,
            BTreeSet::from([graph.file(index).target.clone()]),
        ));
    }
    let mut visited = BTreeSet::new();
    walk_module_graph(
        graph,
        &by_path,
        &mut pending,
        &mut visited,
        &mut assignments,
    );

    let incoming = graph
        .files()
        .iter()
        .enumerate()
        .filter(|(_, fact)| fact.resolution_family == ResolutionFamily::RustCrates)
        .flat_map(|(file, fact)| {
            let by_path = &by_path;
            fact.aliases.iter().flat_map(move |path| {
                module_children(graph, by_path, file, path)
                    .into_iter()
                    .map(|(child, _, _)| child)
            })
        })
        .collect::<BTreeSet<_>>();

    for (index, assignment) in assignments.iter().enumerate() {
        if graph.file(index).resolution_family == ResolutionFamily::RustCrates
            && assignment.is_empty()
            && !incoming.contains(&index)
        {
            for path in graph.file(index).aliases {
                pending.push_back((
                    index,
                    FileContext {
                        crate_root: CrateKey::Detached(graph.file(index).target.clone()),
                        module: Vec::new(),
                    },
                    path.clone(),
                    BTreeSet::from([graph.file(index).target.clone()]),
                ));
            }
        }
    }
    walk_module_graph(
        graph,
        &by_path,
        &mut pending,
        &mut visited,
        &mut assignments,
    );

    graph
        .files()
        .iter()
        .zip(assignments)
        .map(|(fact, contexts)| {
            if fact.resolution_family != ResolutionFamily::RustCrates {
                return Vec::new();
            }
            match contexts.first() {
                Some(first)
                    if contexts
                        .iter()
                        .all(|context| context.crate_root == first.crate_root) =>
                {
                    contexts.into_iter().collect()
                }
                _ => vec![FileContext {
                    crate_root: CrateKey::Detached(fact.target.clone()),
                    module: Vec::new(),
                }],
            }
        })
        .collect()
}

fn walk_module_graph(
    graph: &ProgramGraph<'_>,
    by_path: &BTreeMap<PathBuf, usize>,
    pending: &mut VecDeque<ModuleRoute>,
    visited: &mut BTreeSet<ModuleRoute>,
    assignments: &mut [BTreeSet<FileContext>],
) {
    while let Some((file, context, path, ancestry)) = pending.pop_front() {
        if !visited.insert((file, context.clone(), path.clone(), ancestry.clone())) {
            continue;
        }
        assignments[file].insert(context.clone());
        for (child_file, child_path, module) in module_children(graph, by_path, file, &path) {
            let target = graph.file(child_file).target;
            if ancestry.contains(target) {
                continue;
            }
            let mut child_ancestry = ancestry.clone();
            child_ancestry.insert(target.clone());
            pending.push_back((
                child_file,
                FileContext {
                    crate_root: context.crate_root.clone(),
                    module: joined_path(&context.module, &module),
                },
                child_path,
                child_ancestry,
            ));
        }
    }
}

/// Return declared child modules only when all matching paths name one file.
fn module_children(
    graph: &ProgramGraph<'_>,
    by_path: &BTreeMap<PathBuf, usize>,
    file: usize,
    path: &Path,
) -> Vec<(usize, PathBuf, Vec<String>)> {
    graph
        .definitions(file)
        .filter(|definition| {
            definition.kind == DefinitionKind::Module && definition.external_module
        })
        .flat_map(|declaration| {
            let module = joined_path(&graph.file(file).module.0, &declaration.module.0);
            let candidates = module_files(path, &module, &declaration.name)
                .into_iter()
                .filter_map(|path| by_path.get(&path).map(|file| (path, *file)))
                .collect::<BTreeSet<_>>();
            let Some((_, first)) = candidates.first() else {
                return Vec::new();
            };
            if candidates.iter().any(|(_, file)| file != first) {
                return Vec::new();
            }
            let module = joined_path(&module, std::slice::from_ref(&declaration.name));
            candidates
                .into_iter()
                .map(|(path, file)| (file, path, module.clone()))
                .collect()
        })
        .collect()
}

fn module_files(file: &Path, inline_module: &[String], name: &str) -> Vec<PathBuf> {
    let Some(parent) = file.parent() else {
        return Vec::new();
    };
    let base = if file.extension().is_some_and(|extension| extension == "rs")
        && !file
            .file_name()
            .and_then(|file_name| file_name.to_str())
            .is_some_and(|file_name| matches!(file_name, "mod.rs" | "lib.rs" | "main.rs"))
    {
        file.file_stem()
            .map_or_else(|| parent.to_path_buf(), |stem| parent.join(stem))
    } else {
        parent.to_path_buf()
    };
    let base = inline_module
        .iter()
        .fold(base, |path, part| path.join(part));
    vec![
        base.join(format!("{name}.rs")),
        base.join(name).join("mod.rs"),
    ]
}

fn is_crate_root(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, "lib.rs" | "main.rs"))
}
