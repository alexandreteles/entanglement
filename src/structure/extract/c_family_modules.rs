use super::*;
use crate::languages::c_family::{conditional::Active, project::Project};
use crate::resolver::c_family::{module_files, modules::absolute};
use std::collections::BTreeSet;

pub(super) fn relationships(facts: &[FileFacts], project: &Project) -> Vec<Relationship> {
    if project.units.is_empty() {
        return Vec::new();
    }
    let paths: BTreeMap<_, _> = facts
        .iter()
        .map(|fact| (&fact.target, fact.analysis.path.as_str()))
        .collect();
    let modules = module_files(facts);
    let mut edges = vec![];
    for fact in facts {
        let origin = &fact.analysis.path;
        let mut includes: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
        let included = project.has_includer(&fact.target);
        let views: Vec<_> = project
            .units
            .iter()
            .filter(|unit| !included || !unit.fallback)
            .filter_map(|unit| unit.files.get(&fact.target))
            .collect();
        for view in &views {
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
                    .entry((
                        include.range.start,
                        include.range.end,
                        &include.spelling,
                        include.export,
                    ))
                    .or_default()
                    .insert(outcome);
            }
        }
        for ((start, end, spelling, export), targets) in includes {
            let (resolution, target) =
                if views.iter().any(|view| !view.includes.contains_key(&start)) {
                    (ResolutionStatus::Unresolved, spelling.clone())
                } else if targets.len() == 1 {
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
                export,
            ));
        }
        for data in fact
            .contexts
            .iter()
            .filter_map(|store| store.semantic.c_family.as_deref())
        {
            for (start, end, name, export) in &data.imports {
                if name.starts_with(['"', '<']) {
                    continue;
                }
                let absolute = absolute(data.module.as_deref(), name);
                let outcome = match modules.get(&absolute) {
                    Some(paths) if paths.len() == 1 => (
                        module_id(paths.first().unwrap()),
                        ResolutionStatus::Resolved,
                    ),
                    Some(_) => (absolute, ResolutionStatus::Ambiguous),
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
