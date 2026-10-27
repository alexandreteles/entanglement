use super::*;

pub(super) fn units(facts: &[FileFacts]) -> Vec<Unit> {
    let mut units = Vec::new();
    for fact in facts {
        units.push(Unit {
            id: module_id(&fact.analysis.path),
            name: fact.analysis.path.clone(),
            source_hash: Some(fact.analysis.hash.clone()),
            structural_fingerprint: Some(
                blake3::hash(
                    fact.contexts
                        .iter()
                        .filter_map(|store| {
                            store
                                .semantic
                                .structure_fingerprint
                                .as_ref()
                                .map(|fingerprint| {
                                    format!(
                                        "{}:{fingerprint}",
                                        store
                                            .semantic
                                            .structure_language
                                            .as_deref()
                                            .unwrap_or("unknown")
                                    )
                                })
                        })
                        .collect::<Vec<_>>()
                        .join(";")
                        .as_bytes(),
                )
                .to_hex()
                .to_string(),
            ),
            kind: UnitKind::Module,
            path: fact.analysis.path.clone(),
        });
        units.extend(fact.contexts.iter().flat_map(|store| {
            store
                .semantic
                .structure
                .iter()
                .filter(|record| record.is_type)
                .map(|record| Unit {
                    id: type_id(fact, store, record),
                    name: qualified(record),
                    source_hash: Some(fact.analysis.hash.clone()),
                    structural_fingerprint: record.fingerprint.as_ref().map(|fingerprint| {
                        format!(
                            "{}:{fingerprint}",
                            store
                                .semantic
                                .structure_language
                                .as_deref()
                                .unwrap_or("unknown")
                        )
                    }),
                    kind: UnitKind::Type,
                    path: fact.analysis.path.clone(),
                })
        }));
    }
    units.sort_by(|left, right| left.id.cmp(&right.id));
    units
}

pub(super) fn symbol_targets(facts: &[FileFacts]) -> SymbolTargets {
    facts
        .iter()
        .flat_map(|fact| {
            fact.contexts.iter().flat_map(|store| {
                store.definitions.iter().map(|definition| {
                    (
                        symbol_key(&fact.analysis.path, definition),
                        (
                            module_id(&fact.analysis.path),
                            type_owner(facts, fact, store, definition.start_byte),
                        ),
                    )
                })
            })
        })
        .collect()
}

pub(super) fn coverage(facts: &[FileFacts]) -> Vec<Coverage> {
    facts
        .iter()
        .flat_map(|fact| {
            fact.contexts
                .iter()
                .map(|store| context_coverage(fact, store))
        })
        .collect()
}

fn context_coverage(fact: &FileFacts, store: &SemanticStore) -> Coverage {
    let language = store
        .semantic
        .structure_language
        .as_deref()
        .unwrap_or(&fact.analysis.language);
    let applicable = adapters::object_analysis(language)
        || store.semantic.structure.iter().any(|record| record.is_type);
    let mut limitations = adapters::limitations(language);
    if store.semantic.structure_errors {
        limitations.push("parse_error".to_owned());
    }
    if !applicable {
        limitations.push("no_oo".to_owned());
    }
    let status = if store.semantic.structure_errors {
        CoverageStatus::Unavailable
    } else {
        CoverageStatus::Partial
    };
    let effective = if applicable {
        status
    } else if store.semantic.structure_errors {
        CoverageStatus::Unavailable
    } else {
        CoverageStatus::Inapplicable
    };
    Coverage {
        effective,
        path: fact.analysis.path.clone(),
        language: language.to_owned(),
        status,
        limitations,
    }
}
