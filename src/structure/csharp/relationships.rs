use super::*;
use crate::languages::csharp::facts::Usage;
use crate::model::{Resolution, SymbolId};
use std::collections::HashMap;

struct Target<'a> {
    decl: &'a Decl,
    file: &'a FileFacts,
}
pub(super) fn append(
    files: &[FileFacts],
    project: &Project,
    types: &Types<'_>,
    snapshot: &mut StructureSnapshot,
) {
    let symbols = files
        .iter()
        .flat_map(|file| {
            file.contexts
                .iter()
                .filter_map(|s| s.semantic.csharp.as_deref())
                .flat_map(move |data| {
                    data.declarations.values().map(move |decl| {
                        (
                            (file.analysis.path.as_str(), decl.range.start),
                            Target { decl, file },
                        )
                    })
                })
        })
        .collect::<HashMap<_, _>>();
    let paths = files
        .iter()
        .map(|f| (f.target.as_path(), f.analysis.path.as_str()))
        .collect::<HashMap<_, _>>();
    for file in files {
        let answers = file
            .analysis
            .resolution
            .iter()
            .map(|r| ((r.start_byte, r.end_byte), &r.resolution))
            .collect::<HashMap<_, _>>();
        for data in file
            .contexts
            .iter()
            .filter_map(|s| s.semantic.csharp.as_deref())
        {
            let module = format!("module:{}", file.analysis.path);
            for directive in &data.script {
                let path = file
                    .target
                    .parent()
                    .unwrap_or(Path::new(""))
                    .join(directive.path.replace('\\', "/"));
                let loaded = crate::input::path_identity(&path)
                    .ok()
                    .and_then(|p| paths.get(p.as_path()).copied());
                let (target, status) = if !directive.load {
                    (directive.path.clone(), ResolutionStatus::External)
                } else if let Some(name) = loaded {
                    (format!("module:{name}"), ResolutionStatus::Resolved)
                } else {
                    (directive.path.clone(), ResolutionStatus::Unresolved)
                };
                snapshot.relationships.push(Relationship {
                    source: module.clone(),
                    target,
                    kind: RelationshipKind::Import,
                    contract: false,
                    ordered: false,
                    resolution: status,
                    evidence: vec![evidence(
                        &file.analysis.path,
                        &directive.range,
                        &module,
                        &directive.path,
                    )],
                });
            }
            for usage in data.uses.values() {
                let resolution = answers
                    .get(&(usage.range.start, usage.range.end))
                    .copied()
                    .unwrap_or(&Resolution::Unresolved);
                let (symbol, status) = match resolution {
                    Resolution::Exact(s) => (Some(s), ResolutionStatus::Resolved),
                    Resolution::Ambiguous(_) => (None, ResolutionStatus::Ambiguous),
                    Resolution::External => (None, ResolutionStatus::External),
                    Resolution::Unresolved => (None, ResolutionStatus::Unresolved),
                };
                let target =
                    symbol.and_then(|s: &SymbolId| symbols.get(&(s.file.as_str(), s.start_byte)));
                let kind = if usage.relation == RelationshipKind::Inherit
                    && target.is_some_and(|s| s.decl.category == "interface")
                    && !data
                        .declarations
                        .values()
                        .any(|d| d.category == "interface" && d.qualified() == usage.owner)
                {
                    RelationshipKind::Implement
                } else {
                    usage.relation
                };
                let destination = target.map_or_else(
                    || usage.name.clone(),
                    |t| format!("module:{}", t.file.analysis.path),
                );
                let mut add = |from: &str, to: String| {
                    snapshot
                        .relationships
                        .push(edge(file, usage, from, to, kind, status))
                };
                add(&module, destination);
                for assembly in assemblies(project, &file.target) {
                    let source = unit(assembly, &usage.owner);
                    if !types.contains_key(&source) {
                        continue;
                    }
                    if let Some(symbol) = target {
                        let owners = assemblies(project, &symbol.file.target);
                        let name = if symbol.decl.is_type {
                            symbol.decl.qualified()
                        } else {
                            symbol.decl.owner.clone()
                        };
                        for target_project in owners.iter().filter(|p| {
                            if owners.contains(assembly) {
                                **p == assembly
                            } else {
                                project.owners(&file.target).iter().any(|v| {
                                    project.views[*v].project == assembly
                                        && project.reachable[*v]
                                            .iter()
                                            .any(|r| project.views[*r].project == ***p)
                                })
                            }
                        }) {
                            let id = unit(target_project, &name);
                            if types.contains_key(&id) {
                                add(&source, id);
                            }
                        }
                    } else {
                        add(&source, usage.name.clone());
                    }
                }
            }
        }
    }
}
fn edge(
    file: &FileFacts,
    usage: &Usage,
    source: &str,
    target: String,
    kind: RelationshipKind,
    resolution: ResolutionStatus,
) -> Relationship {
    Relationship {
        source: source.into(),
        target,
        kind,
        contract: usage.contract,
        ordered: false,
        resolution,
        evidence: vec![evidence(
            &file.analysis.path,
            &usage.range,
            source,
            &usage.name,
        )],
    }
}
