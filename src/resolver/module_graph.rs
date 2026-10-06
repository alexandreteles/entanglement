use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use crate::languages::ResolutionFamily;
use crate::model::{DefinitionKind, FileFacts};

use super::{CrateKey, FileContext, joined_path};

type ModuleRoute = (usize, FileContext, PathBuf, BTreeSet<PathBuf>);

pub(super) fn file_contexts(facts: &[FileFacts]) -> Vec<Vec<FileContext>> {
    let mut by_path = BTreeMap::<PathBuf, usize>::new();
    for (index, fact) in facts.iter().enumerate() {
        if fact.resolution_family != ResolutionFamily::RustCrates {
            continue;
        }
        for path in &fact.aliases {
            by_path.insert(path.clone(), index);
        }
    }
    let mut roots = facts
        .iter()
        .enumerate()
        .flat_map(|(index, fact)| {
            if fact.resolution_family != ResolutionFamily::RustCrates {
                return Vec::new();
            }
            fact.aliases
                .iter()
                .filter(|path| is_crate_root(path))
                .map(move |path| (path.clone(), index))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    roots.sort_by(|left, right| left.0.cmp(&right.0));

    let mut assignments = vec![BTreeSet::new(); facts.len()];
    let mut pending = VecDeque::new();
    for (root_path, index) in roots {
        pending.push_back((
            index,
            FileContext {
                crate_root: CrateKey::Logical(root_path.clone()),
                module: Vec::new(),
            },
            root_path,
            BTreeSet::from([facts[index].target.clone()]),
        ));
    }
    let mut visited = BTreeSet::new();
    walk_module_graph(
        facts,
        &by_path,
        &mut pending,
        &mut visited,
        &mut assignments,
    );

    let mut incoming = BTreeSet::new();
    for fact in facts {
        if fact.resolution_family != ResolutionFamily::RustCrates {
            continue;
        }
        for path in &fact.aliases {
            for declaration in fact.definitions.iter().filter(|definition| {
                definition.kind == DefinitionKind::Module && definition.external_module
            }) {
                let relative_module = joined_path(&fact.module.0, &declaration.module.0);
                let targets = module_files(path, &relative_module, &declaration.name)
                    .iter()
                    .filter_map(|path| by_path.get(path))
                    .copied()
                    .collect::<BTreeSet<_>>();
                if targets.len() == 1 {
                    incoming.insert(*targets.first().expect("One target is present"));
                }
            }
        }
    }

    for index in 0..facts.len() {
        if facts[index].resolution_family != ResolutionFamily::RustCrates {
            continue;
        }
        if assignments[index].is_empty() && !incoming.contains(&index) {
            for path in &facts[index].aliases {
                pending.push_back((
                    index,
                    FileContext {
                        crate_root: CrateKey::Detached(facts[index].target.clone()),
                        module: Vec::new(),
                    },
                    path.clone(),
                    BTreeSet::from([facts[index].target.clone()]),
                ));
            }
        }
    }
    walk_module_graph(
        facts,
        &by_path,
        &mut pending,
        &mut visited,
        &mut assignments,
    );

    facts
        .iter()
        .enumerate()
        .map(|(index, fact)| {
            if fact.resolution_family != ResolutionFamily::RustCrates {
                return Vec::new();
            }
            let contexts = assignments[index].iter().cloned().collect::<Vec<_>>();
            let same_crate = contexts.first().is_some_and(|first| {
                contexts
                    .iter()
                    .all(|context| context.crate_root == first.crate_root)
            });
            if same_crate {
                contexts
            } else {
                vec![FileContext {
                    crate_root: CrateKey::Detached(fact.target.clone()),
                    module: Vec::new(),
                }]
            }
        })
        .collect()
}

fn walk_module_graph(
    facts: &[FileFacts],
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
        let file_module = joined_path(&context.module, &facts[file].module.0);
        for declaration in facts[file].definitions.iter().filter(|definition| {
            definition.kind == DefinitionKind::Module && definition.external_module
        }) {
            let module = joined_path(&file_module, &declaration.module.0);
            let child_module = joined_path(&module, std::slice::from_ref(&declaration.name));
            let relative_module = joined_path(&facts[file].module.0, &declaration.module.0);
            let candidates = module_files(&path, &relative_module, &declaration.name)
                .into_iter()
                .filter_map(|path| by_path.get(&path).map(|file| (path, *file)))
                .collect::<BTreeSet<_>>();
            let targets = candidates
                .iter()
                .map(|(_, child_file)| child_file)
                .collect::<BTreeSet<_>>();
            if targets.len() == 1 {
                for (child_path, child_file) in candidates {
                    let target = &facts[child_file].target;
                    if ancestry.contains(target) {
                        continue;
                    }
                    let mut child_ancestry = ancestry.clone();
                    child_ancestry.insert(target.clone());
                    pending.push_back((
                        child_file,
                        FileContext {
                            crate_root: context.crate_root.clone(),
                            module: child_module.clone(),
                        },
                        child_path,
                        child_ancestry,
                    ));
                }
            }
        }
    }
}

fn module_files(file: &Path, inline_module: &[String], name: &str) -> Vec<PathBuf> {
    let Some(parent) = file.parent() else {
        return Vec::new();
    };
    let base = if file.file_name().and_then(|file_name| file_name.to_str()) == Some("mod.rs") {
        parent.to_path_buf()
    } else if file.extension().is_some_and(|extension| extension == "rs")
        && !file
            .file_name()
            .and_then(|file_name| file_name.to_str())
            .is_some_and(|file_name| matches!(file_name, "lib.rs" | "main.rs"))
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
