use super::*;

fn target(
    fact: &FileFacts,
    reference: &Reference,
    symbols: &SymbolTargets,
    resolved: Option<&crate::model::ReferenceAnalysis>,
) -> (ResolutionStatus, String, Option<String>) {
    let written = reference.path.join("::");
    if fact.resolution_family == crate::languages::ResolutionFamily::CFamily
        && matches!(reference.kind, ReferenceKind::Call | ReferenceKind::Method)
        && resolved.is_some_and(|item| {
            matches!(item.resolution, Resolution::Exact(_))
                && item.dispatch != Some(crate::graph::CallDispatch::Direct)
        })
    {
        return (ResolutionStatus::Unresolved, written, None);
    }
    match resolved.map(|item| &item.resolution) {
        Some(Resolution::Exact(symbol)) => match symbols.get(&resolved_key(symbol)) {
            Some((module, target)) => (ResolutionStatus::Resolved, module.clone(), target.clone()),
            None => (ResolutionStatus::Unresolved, written, None),
        },
        Some(Resolution::External) => (ResolutionStatus::External, written, None),
        Some(Resolution::Ambiguous(_)) => (ResolutionStatus::Ambiguous, written, None),
        _ => (ResolutionStatus::Unresolved, written, None),
    }
}

pub(super) fn reference_relationships(
    facts: &[FileFacts],
    fact: &FileFacts,
    store: &SemanticStore,
    symbols: &SymbolTargets,
    reference: &Reference,
) -> Vec<Relationship> {
    if reference.kind == ReferenceKind::Macro {
        return Vec::new();
    }
    let source_module = module_id(&fact.analysis.path);
    let resolved = fact.analysis.resolution.iter().find(|item| {
        item.start_byte == reference.start_byte
            && item.end_byte == reference.end_byte
            && item.path == reference.path
    });
    let (resolution, target_module, target_type) = target(fact, reference, symbols, resolved);
    let owner = store
        .semantic
        .structure
        .iter()
        .filter(|record| record.start <= reference.start_byte && reference.end_byte <= record.end)
        .min_by_key(|record| (record.end - record.start, !record.public));
    let special = store
        .semantic
        .structure
        .iter()
        .filter(|record| record.start <= reference.start_byte && reference.end_byte <= record.end)
        .flat_map(|record| &record.relations)
        .filter(|relation| {
            relation.start <= reference.start_byte && reference.end_byte <= relation.end
        })
        .min_by_key(|relation| relation.end - relation.start);
    let kind = special
        .map(|relation| relation.kind)
        .unwrap_or(match reference.kind {
            ReferenceKind::Call | ReferenceKind::Method => RelationshipKind::Call,
            ReferenceKind::Type => RelationshipKind::Type,
            ReferenceKind::MemberInitializer if resolved.is_some_and(|item| {
                matches!(&item.resolution, Resolution::Exact(symbol) if symbol.kind == crate::model::DefinitionKind::Other)
            }) => RelationshipKind::Field,
            ReferenceKind::Qualified | ReferenceKind::Value if reference.access.is_some() => {
                RelationshipKind::Field
            }
            _ => RelationshipKind::Type,
        });
    let ordered = special.is_some_and(|relation| relation.ordered);
    let contract = store.semantic.c_family.is_none()
        && owner.is_some_and(|owner| owner.public && special.is_some())
        || store.semantic.structure.iter().any(|record| {
            record.public
                && record.type_ranges.iter().any(|(start, end)| {
                    *start <= reference.start_byte && reference.end_byte <= *end
                })
        });
    let evidence = reference_evidence(fact, store, reference, owner, special);
    let mut relationships = vec![Relationship {
        ordered: false,
        source: source_module,
        target: target_module,
        kind,
        contract,
        resolution,
        evidence: evidence.clone(),
    }];
    if let Some(source) = type_owner(facts, fact, store, reference.start_byte) {
        if let Some(target) = target_type {
            relationships.push(Relationship {
                ordered,
                source,
                target,
                kind,
                contract,
                resolution,
                evidence,
            });
        } else if resolution != ResolutionStatus::Resolved {
            relationships.push(Relationship {
                ordered,
                source,
                target: reference.path.join("::"),
                kind,
                contract,
                resolution,
                evidence,
            });
        }
    }
    relationships
}

fn reference_evidence(
    fact: &FileFacts,
    store: &SemanticStore,
    reference: &Reference,
    owner: Option<&SyntaxDeclaration>,
    special: Option<&SyntaxRelation>,
) -> Vec<Evidence> {
    let owner_id = owner.map_or_else(
        || module_id(&fact.analysis.path),
        |owner| {
            format!(
                "{}#{}:{}",
                fact.analysis.path,
                store.semantic.context_id,
                qualified(owner)
            )
        },
    );
    let mut evidence = vec![Evidence {
        path: fact.analysis.path.clone(),
        start: reference.start_byte,
        end: reference.end_byte,
        owner: owner_id,
        detail: reference.path.join("::"),
    }];
    if let Some(relation) = special {
        evidence.push(Evidence {
            path: fact.analysis.path.clone(),
            start: relation.start,
            end: relation.end,
            owner: evidence[0].owner.clone(),
            detail: if store
                .semantic
                .c_family
                .as_ref()
                .is_some_and(|data| data.virtual_bases.contains(&relation.start))
            {
                "virtual_base"
            } else if relation.kind == RelationshipKind::Compose
                && owner.is_some_and(|owner| owner.receiver.as_deref() == Some("embed_pointer"))
            {
                "embed_ptr"
            } else {
                "relation"
            }
            .to_owned(),
        });
    }
    evidence
}
