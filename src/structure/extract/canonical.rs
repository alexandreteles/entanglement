use super::*;

pub(super) fn canonical_types(
    facts: &[FileFacts],
    source_snapshot: &crate::snapshot::Snapshot,
    symbols: &SymbolTargets,
    relationships: &mut Vec<Relationship>,
) {
    for relation in crate::resolver::structural_type_relationships(facts, source_snapshot) {
        let Some(fact) = facts
            .iter()
            .find(|fact| fact.analysis.path == relation.owner.file)
        else {
            continue;
        };
        let Some(source) = symbols
            .get(&resolved_key(&relation.owner))
            .and_then(|(_, target)| target.clone())
        else {
            continue;
        };
        let record = fact
            .contexts
            .iter()
            .flat_map(|store| &store.semantic.structure)
            .find(|record| {
                record.is_type
                    && record.start <= relation.owner.start_byte
                    && relation.owner.start_byte < record.end
            });
        let range = record
            .and_then(|record| {
                record.relations.iter().find(|range| {
                    range.kind == relation.kind
                        && range.start <= relation.start
                        && relation.start < range.end
                })
            })
            .map(|range| (range.start, range.end))
            .or_else(|| record.map(|record| (record.start, record.end)))
            .unwrap_or((relation.start, relation.start));
        let (resolution, target_module, target_type) = match relation.target {
            Resolution::Exact(symbol) => match symbols.get(&resolved_key(&symbol)) {
                Some((module, target)) => {
                    (ResolutionStatus::Resolved, module.clone(), target.clone())
                }
                None => (ResolutionStatus::Unresolved, relation.name.clone(), None),
            },
            Resolution::External => (ResolutionStatus::External, relation.name.clone(), None),
            Resolution::Ambiguous(_) => (ResolutionStatus::Ambiguous, relation.name.clone(), None),
            Resolution::Unresolved => (ResolutionStatus::Unresolved, relation.name.clone(), None),
        };
        let contract = record.is_some_and(|record| record.public);
        let evidence = vec![Evidence {
            path: fact.analysis.path.clone(),
            start: range.0,
            end: range.1,
            owner: source.clone(),
            detail: relation.name.clone(),
        }];
        relationships.push(Relationship {
            ordered: false,
            source: module_id(&fact.analysis.path),
            target: target_module,
            kind: relation.kind,
            contract,
            resolution,
            evidence: evidence.clone(),
        });
        relationships.push(Relationship {
            ordered: false,
            source,
            target: target_type.unwrap_or(relation.name),
            kind: relation.kind,
            contract,
            resolution,
            evidence,
        });
    }
}

pub(super) fn canonical_modules(facts: &[FileFacts], relationships: &mut Vec<Relationship>) {
    for (file, start, end, source, resolution) in crate::resolver::structural_modules(facts) {
        let fact = &facts[file];
        let module = module_id(&fact.analysis.path);
        relationships.retain(|edge| {
            !(edge.source == module
                && matches!(
                    edge.kind,
                    RelationshipKind::Import | RelationshipKind::Reexport
                )
                && edge
                    .evidence
                    .iter()
                    .any(|evidence| evidence.start == start && evidence.end == end))
        });
        let (target, status) = match resolution {
            Resolution::Exact(symbol) => (module_id(&symbol.file), ResolutionStatus::Resolved),
            Resolution::External => (source.clone(), ResolutionStatus::External),
            Resolution::Ambiguous(_) => (source.clone(), ResolutionStatus::Ambiguous),
            Resolution::Unresolved => (source.clone(), ResolutionStatus::Unresolved),
        };
        let reexport = fact
            .contexts
            .iter()
            .flat_map(|store| &store.exports)
            .any(|export| export.start_byte == start && export.end_byte == end);
        relationships.push(Relationship {
            ordered: false,
            source: module.clone(),
            target,
            kind: if reexport {
                RelationshipKind::Reexport
            } else {
                RelationshipKind::Import
            },
            contract: reexport,
            resolution: status,
            evidence: vec![Evidence {
                path: fact.analysis.path.clone(),
                start,
                end,
                owner: module,
                detail: source,
            }],
        });
    }
}
