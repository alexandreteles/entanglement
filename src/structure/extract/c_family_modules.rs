use super::*;
use crate::languages::c_family::{conditional::Active, project::Project};
use crate::resolver::c_family::{module_files, modules::absolute};
use std::collections::BTreeSet;

pub(super) fn relationships(facts: &[FileFacts], project: &Project) -> Vec<Relationship> {
    let paths: BTreeMap<_, _> = facts
        .iter()
        .map(|fact| (&fact.target, fact.analysis.path.as_str()))
        .collect();
    let modules = module_files(facts);
    let mut edges = vec![];
    for fact in facts {
        let origin = &fact.analysis.path;
        let mut includes: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
        for view in project
            .units
            .iter()
            .filter_map(|unit| unit.files.get(&fact.target))
        {
            for include in view.includes.values() {
                let found = include.target.as_ref().and_then(|path| paths.get(path));
                let outcome = match found {
                    Some(path) if include.active == Active::Yes => {
                        (ResolutionStatus::Resolved, module_id(path))
                    }
                    None if include.system || include.target.is_some() => {
                        (ResolutionStatus::External, include.spelling.clone())
                    }
                    _ => (ResolutionStatus::Unresolved, include.spelling.clone()),
                };
                includes
                    .entry((include.range.start, include.range.end, &include.spelling))
                    .or_default()
                    .insert(outcome);
            }
        }
        for ((start, end, spelling), targets) in includes {
            let (resolution, target) = if targets.len() == 1 {
                targets.into_iter().next().unwrap()
            } else {
                (ResolutionStatus::Ambiguous, spelling.clone())
            };
            edges.push(edge(
                origin,
                start,
                end,
                spelling,
                (target, resolution),
                false,
            ));
        }
        for data in fact
            .contexts
            .iter()
            .filter_map(|store| store.semantic.c_family.as_deref())
        {
            for (start, end, name, export) in &data.imports {
                let absolute = absolute(data.module.as_deref(), name);
                let header = name
                    .strip_prefix('"')
                    .and_then(|name| name.strip_suffix('"'))
                    .and_then(|name| fact.target.parent()?.join(name).canonicalize().ok())
                    .and_then(|path| paths.get(&path).copied());
                let outcome = match modules.get(&absolute) {
                    Some(paths) if paths.len() == 1 => (
                        module_id(paths.first().unwrap()),
                        ResolutionStatus::Resolved,
                    ),
                    Some(_) => (absolute, ResolutionStatus::Ambiguous),
                    None if header.is_some() => {
                        (module_id(header.unwrap()), ResolutionStatus::Resolved)
                    }
                    None if name.starts_with('<') => (absolute, ResolutionStatus::External),
                    None => (absolute, ResolutionStatus::Unresolved),
                };
                edges.push(edge(origin, *start, *end, name, outcome, *export));
            }
        }
    }
    edges
}

fn edge(
    path: &str,
    start: usize,
    end: usize,
    detail: &str,
    (target, resolution): (String, ResolutionStatus),
    export: bool,
) -> Relationship {
    let owner = module_id(path);
    Relationship {
        source: owner.clone(),
        target,
        resolution,
        ordered: false,
        contract: export,
        kind: if export {
            RelationshipKind::Reexport
        } else {
            RelationshipKind::Import
        },
        evidence: vec![Evidence {
            path: path.into(),
            start,
            end,
            owner,
            detail: detail.into(),
        }],
    }
}
