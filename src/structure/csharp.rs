mod relationships;
mod surfaces;
use super::{effective::EffectiveDeclaration, model::*};
use crate::languages::csharp::{facts::Decl, project::Project};
use crate::model::FileFacts;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Clone, Copy)]
struct Part<'a> {
    file: &'a FileFacts,
    decl: &'a Decl,
}
#[derive(Default)]
struct Type<'a> {
    parts: Vec<Part<'a>>,
    members: Vec<Part<'a>>,
}
type Types<'a> = BTreeMap<String, Type<'a>>;

fn assemblies<'a>(project: &'a Project, path: &Path) -> BTreeSet<&'a Path> {
    let mut owners = project
        .owners(path)
        .iter()
        .map(|id| project.views[*id].project.as_path())
        .collect::<BTreeSet<_>>();
    if owners.is_empty() {
        owners.insert(Path::new("<detached>"));
    }
    owners
}
fn unit(assembly: &Path, name: &str) -> String {
    format!(
        "type:{}#source.cs:{}",
        assembly.display(),
        name.replace('.', "::")
    )
}

fn evidence(path: &str, range: &std::ops::Range<usize>, owner: &str, detail: &str) -> Evidence {
    Evidence {
        path: path.into(),
        start: range.start,
        end: range.end,
        owner: owner.into(),
        detail: detail.into(),
    }
}

pub(super) fn collect(
    files: &[FileFacts],
    project: &Project,
    snapshot: &mut StructureSnapshot,
) -> Vec<EffectiveDeclaration> {
    let mut types = Types::new();
    let mut coverage = BTreeMap::new();
    for file in files {
        let owners = assemblies(project, &file.target);
        for data in file
            .contexts
            .iter()
            .filter_map(|s| s.semantic.csharp.as_deref())
        {
            let limits = coverage
                .entry(file.analysis.path.as_str())
                .or_insert_with(BTreeSet::new);
            for owner in project.owners(&file.target) {
                limits.extend(project.views[*owner].limits.iter().cloned());
            }
            limits.extend(
                data.errors
                    .iter()
                    .map(|r| format!("parse_error:{}..{}", r.start, r.end)),
            );
            for owner in &owners {
                for decl in data.declarations.values() {
                    if !project.owners(&file.target).is_empty()
                        && !project.owners(&file.target).iter().any(|v| {
                            project.views[*v].project == **owner
                                && project.active(&file.target, *v, decl.range.start)
                        })
                    {
                        continue;
                    }
                    let part = Part { file, decl };
                    if decl.is_type {
                        types
                            .entry(unit(owner, &decl.qualified()))
                            .or_default()
                            .parts
                            .push(part);
                    } else if decl.category != "local" {
                        types
                            .entry(unit(owner, &decl.owner))
                            .or_default()
                            .members
                            .push(part);
                    }
                }
            }
        }
    }
    if coverage.is_empty() {
        return Vec::new();
    }
    for row in &mut snapshot.coverage {
        if row.language == "source.cs"
            && let Some(limits) = coverage.get(row.path.as_str())
        {
            row.limitations
                .retain(|l| !matches!(l.as_str(), "no_oo" | "unsupported"));
            row.limitations.extend(limits.iter().cloned());
            row.limitations.sort();
            row.limitations.dedup();
            row.effective = row.status;
        }
    }
    types.retain(|_, ty| !ty.parts.is_empty());
    for (id, ty) in &types {
        let first = ty.parts[0];
        let mut hash = blake3::Hasher::new();
        for part in &ty.parts {
            hash.update(&part.file.source[part.decl.range.clone()]);
        }
        let fingerprints = ty
            .parts
            .iter()
            .map(|part| part.decl.fingerprint.as_deref())
            .collect::<Option<Vec<_>>>();
        let fingerprint = fingerprints.map(|mut parts| {
            parts.sort_unstable();
            format!(
                "source.cs:{}",
                blake3::hash(parts.join("/").as_bytes()).to_hex()
            )
        });
        snapshot.units.push(Unit {
            id: id.clone(),
            name: first.decl.qualified(),
            path: first.file.analysis.path.clone(),
            kind: UnitKind::Type,
            source_hash: Some(hash.finalize().to_hex().to_string()),
            structural_fingerprint: fingerprint,
        });
    }
    snapshot.units.sort_by(|a, b| a.id.cmp(&b.id));
    relationships::append(files, project, &types, snapshot);
    snapshot.relationships =
        super::extract::deduplicate(std::mem::take(&mut snapshot.relationships));
    surfaces::build(types, snapshot, project)
}
