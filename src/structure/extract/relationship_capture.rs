use super::references::reference_relationships;
use super::*;

pub(super) fn capture_relationships(
    facts: &[FileFacts],
    fact: &FileFacts,
    store: &SemanticStore,
    symbols: &SymbolTargets,
) -> Vec<Relationship> {
    store
        .references
        .iter()
        .flat_map(|reference| reference_relationships(facts, fact, store, symbols, reference))
        .chain(
            store
                .imports
                .iter()
                .map(|import| import_relationship(fact, store, import)),
        )
        .collect()
}

fn import_relationship(
    fact: &FileFacts,
    store: &SemanticStore,
    import: &crate::model::Import,
) -> Relationship {
    let source_module = module_id(&fact.analysis.path);
    let name = import
        .alias
        .as_deref()
        .or(import.imported_name.as_deref())
        .or_else(|| import.path.last().map(String::as_str));
    let used = name
        .and_then(|name| {
            store
                .references
                .iter()
                .find(|reference| reference.path.first().is_some_and(|part| part == name))
        })
        .and_then(|reference| {
            fact.analysis.resolution.iter().find(|item| {
                item.start_byte == reference.start_byte
                    && matches!(item.resolution, Resolution::Exact(_))
            })
        });
    let target = used.and_then(|reference| match &reference.resolution {
        Resolution::Exact(symbol) => Some(module_id(&symbol.file)),
        _ => None,
    });
    Relationship {
        ordered: false,
        source: source_module.clone(),
        resolution: if target.is_some() {
            ResolutionStatus::Resolved
        } else {
            ResolutionStatus::Unresolved
        },
        target: target.unwrap_or_else(|| {
            import
                .source
                .clone()
                .unwrap_or_else(|| import.path.join("::"))
        }),
        kind: if import.is_public {
            RelationshipKind::Reexport
        } else {
            RelationshipKind::Import
        },
        contract: import.is_public,
        evidence: vec![Evidence {
            path: fact.analysis.path.clone(),
            start: import.start_byte,
            end: import.end_byte,
            owner: format!("{source_module}#{}", store.semantic.context_id),
            detail: import.path.join("::"),
        }],
    }
}
