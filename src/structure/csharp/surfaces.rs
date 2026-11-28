use super::*;
use crate::structure::effective::EffectiveMember;
use crate::structure::types::TypeStructure;

pub(super) fn build(
    types: Types<'_>,
    snapshot: &StructureSnapshot,
    project: &Project,
) -> Vec<EffectiveDeclaration> {
    let mut direct = BTreeMap::new();
    for (id, ty) in &types {
        let mut members = BTreeMap::<String, EffectiveMember>::new();
        let mut conflicts = BTreeSet::new();
        let mut written = Vec::new();
        if ty.parts.len() > 1
            && ty
                .parts
                .iter()
                .any(|p| !p.decl.modifiers.contains("partial"))
            && simultaneous(&ty.parts, project)
        {
            conflicts.insert(format!("duplicate type {}", ty.parts[0].decl.qualified()));
        }
        let interface = ty.parts[0].decl.category == "interface";
        let mut declarations = BTreeMap::<String, Vec<Part<'_>>>::new();
        for part in ty.parts.iter().chain(&ty.members) {
            let d = part.decl;
            let signature = d.signature();
            let origin = if d.is_type {
                id.clone()
            } else {
                format!("{id}::{signature}")
            };
            if d.is_type || d.public() {
                written.extend(d.types.iter().cloned().map(|written| TypeStructure {
                    written,
                    expanded: None,
                    origin: origin.clone(),
                }));
            }
            if d.is_type {
                continue;
            }
            declarations
                .entry(signature.clone())
                .or_default()
                .push(*part);
            let item = evidence(&part.file.analysis.path, &d.range, &origin, &d.category);
            let m = members.entry(signature).or_insert_with(|| EffectiveMember {
                name: d.name.clone(),
                required: d.modifiers.contains("required")
                    || d.modifiers.contains("abstract")
                    || interface && d.body.is_none(),
                optional: d.modifiers.contains("partial")
                    && d.body.is_none()
                    && d.visibility == "private"
                    && d.ty == "void",
                receiver: d.extension.clone(),
                member_kind: Some(d.category.clone()),
                origin: origin.clone(),
                contracts: Vec::new(),
                visibility: d.visibility.clone(),
                body: None,
                selection: None,
                provenance: vec![id.clone()],
                evidence: Vec::new(),
            });
            m.evidence.push(item);
            if d.body.is_some() {
                m.optional = false;
                m.body = Some(origin);
            }
        }
        for (signature, parts) in declarations {
            if parts.len() > 1
                && simultaneous(&parts, project)
                && !(parts.iter().all(|p| p.decl.modifiers.contains("partial"))
                    && parts.iter().filter(|p| p.decl.body.is_some()).count() <= 1)
            {
                conflicts.insert(format!("duplicate member {signature}"));
            }
        }
        direct.insert(id.clone(), (members, written, conflicts));
    }
    let mut parents = BTreeMap::<&str, Vec<&str>>::new();
    for edge in &snapshot.relationships {
        if matches!(
            edge.kind,
            RelationshipKind::Implement | RelationshipKind::Inherit
        ) && edge.resolution == ResolutionStatus::Resolved
            && types.contains_key(&edge.target)
        {
            parents.entry(&edge.source).or_default().push(&edge.target);
        }
    }
    types
        .iter()
        .map(|(id, ty)| {
            let (mut members, written, conflicts) = direct[id].clone();
            let mut obligations = BTreeSet::new();
            let mut pending = parents.get(id.as_str()).cloned().unwrap_or_default();
            let mut seen = BTreeSet::from([id.as_str()]);
            while let Some(parent) = pending.pop() {
                if !seen.insert(parent) {
                    continue;
                }
                for (signature, member) in &direct[parent].0 {
                    if member.visibility == "private"
                        || matches!(
                            member.member_kind.as_deref(),
                            Some("constructor" | "finalizer")
                        )
                    {
                        continue;
                    }
                    let parent_type = types[parent].parts[0].decl;
                    let explicit = [
                        format!("{}.{signature}", parent_type.name),
                        format!("{}.{signature}", parent_type.qualified()),
                    ];
                    let key = explicit
                        .iter()
                        .find(|s| members.contains_key(*s))
                        .unwrap_or(signature);
                    if let Some(local) = members.get_mut(key) {
                        if member.required {
                            if parent_type.category != "interface"
                                || key != signature
                                || local.visibility == "public"
                            {
                                local.contracts.push(member.origin.clone());
                            } else {
                                obligations.insert(member.origin.clone());
                            }
                        }
                    } else {
                        let mut inherited = member.clone();
                        inherited.provenance.insert(0, id.clone());
                        if member.required && member.body.is_none() {
                            obligations.insert(member.origin.clone());
                        }
                        members.insert(signature.clone(), inherited);
                    }
                }
                pending.extend(parents.get(parent).into_iter().flatten());
            }
            let evidence = ty
                .parts
                .iter()
                .map(|p| evidence(&p.file.analysis.path, &p.decl.range, id, &p.decl.category))
                .collect();
            EffectiveDeclaration {
                unit: id.clone(),
                evidence,
                status: if conflicts.is_empty() {
                    CoverageStatus::Partial
                } else {
                    CoverageStatus::Unavailable
                },
                members: members.into_values().collect(),
                types: written,
                obligations: obligations.into_iter().collect(),
                conflicts: conflicts.into_iter().collect(),
                satisfies: Vec::new(),
            }
        })
        .collect()
}
fn simultaneous(parts: &[Part<'_>], project: &Project) -> bool {
    parts.iter().enumerate().any(|(i, p)| {
        parts[..i].iter().any(|other| {
            let owners = project.owners(&p.file.target);
            owners.is_empty()
                || owners.iter().any(|v| {
                    project.owners(&other.file.target).contains(v)
                        && project.active(&p.file.target, *v, p.decl.range.start)
                        && project.active(&other.file.target, *v, other.decl.range.start)
                })
        })
    })
}
