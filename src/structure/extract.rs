mod fingerprint;
mod ownership;
pub(crate) mod signatures;
pub(crate) use ownership::type_owner;
mod adapters;
mod capture;
use std::collections::BTreeMap;

use super::model::*;
use super::types::TypeExpression;
use crate::model::{
    Definition, FileFacts, Reference, ReferenceKind, Resolution, SemanticStore, SymbolId,
};
pub(crate) use capture::capture;

#[derive(Clone, Debug)]
pub(crate) struct SyntaxRelation {
    pub start: usize,
    pub end: usize,
    pub kind: RelationshipKind,
    pub ordered: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct SyntaxDeclaration {
    pub start: usize,
    pub end: usize,
    pub body_start: Option<usize>,
    pub name: String,
    pub fingerprint: Option<String>,
    pub owner: Vec<String>,
    pub kind: String,
    pub is_type: bool,
    pub public: bool,
    pub required: bool,
    pub optional: bool,
    pub receiver: Option<String>,
    pub impl_trait: Option<String>,
    pub signature: Option<signatures::MethodSignature>,
    pub member_kind: Option<String>,
    pub types: Vec<TypeExpression>,
    pub type_ranges: Vec<(usize, usize)>,
    pub relations: Vec<SyntaxRelation>,
    pub alias: Option<TypeExpression>,
}

type SymbolTargets = BTreeMap<(String, usize, String), (String, Option<String>)>;

/// Build module and type facts from the canonical stores and resolved references.
/// Embedded contexts retain their existing context IDs. Containment is not an edge.
pub(crate) fn snapshot(
    facts: &[FileFacts],
    scope: &str,
    source_snapshot: &crate::snapshot::Snapshot,
) -> StructureSnapshot {
    let symbols = symbol_targets(facts);
    let relationships = facts
        .iter()
        .flat_map(|fact| {
            fact.contexts
                .iter()
                .flat_map(|store| capture_relationships(facts, fact, store, &symbols))
        })
        .collect();
    let mut result = StructureSnapshot {
        scope: scope.to_owned(),
        units: units(facts),
        coverage: coverage(facts),
        relationships,
        ..Default::default()
    };
    canonical_modules(facts, &mut result.relationships);
    canonical_types(facts, source_snapshot, &symbols, &mut result.relationships);
    result
        .relationships
        .extend(super::resources::relationships(facts));
    result.relationships = deduplicate(result.relationships);
    result
}

fn units(facts: &[FileFacts]) -> Vec<Unit> {
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

fn symbol_targets(facts: &[FileFacts]) -> SymbolTargets {
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

fn coverage(facts: &[FileFacts]) -> Vec<Coverage> {
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

fn canonical_types(
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

fn canonical_modules(facts: &[FileFacts], relationships: &mut Vec<Relationship>) {
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

fn capture_relationships(
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

fn target(
    fact: &FileFacts,
    reference: &Reference,
    symbols: &SymbolTargets,
) -> (ResolutionStatus, String, Option<String>) {
    let resolved = fact.analysis.resolution.iter().find(|item| {
        item.start_byte == reference.start_byte
            && item.end_byte == reference.end_byte
            && item.path == reference.path
    });
    let written = reference.path.join("::");
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

fn reference_relationships(
    facts: &[FileFacts],
    fact: &FileFacts,
    store: &SemanticStore,
    symbols: &SymbolTargets,
    reference: &Reference,
) -> Vec<Relationship> {
    let source_module = module_id(&fact.analysis.path);
    let (resolution, target_module, target_type) = target(fact, reference, symbols);
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
            ReferenceKind::Qualified | ReferenceKind::Value if reference.access.is_some() => {
                RelationshipKind::Field
            }
            _ => RelationshipKind::Type,
        });
    let ordered = special.is_some_and(|relation| relation.ordered);
    let contract = owner.is_some_and(|owner| owner.public && special.is_some())
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
            detail: if relation.kind == RelationshipKind::Compose
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

pub(crate) fn containing_type<'a>(
    _fact: &FileFacts,
    store: &'a SemanticStore,
    position: usize,
) -> Option<&'a SyntaxDeclaration> {
    let enclosing = store
        .semantic
        .structure
        .iter()
        .filter(|record| record.start <= position && position < record.end)
        .min_by_key(|record| record.end - record.start)?;
    if enclosing.is_type {
        return Some(enclosing);
    }
    store
        .semantic
        .structure
        .iter()
        .filter(|record| {
            record.is_type
                && (record.start <= position && position < record.end
                    || enclosing
                        .owner
                        .last()
                        .is_some_and(|owner| owner == &record.name))
        })
        .min_by_key(|record| record.end - record.start)
}

pub(crate) fn type_id(
    fact: &FileFacts,
    store: &SemanticStore,
    record: &SyntaxDeclaration,
) -> String {
    format!(
        "type:{}#{}:{}",
        fact.analysis.path,
        store
            .semantic
            .structure_language
            .as_deref()
            .unwrap_or("unknown"),
        qualified(record)
    )
}

pub(crate) fn qualified(record: &SyntaxDeclaration) -> String {
    record
        .owner
        .iter()
        .chain(std::iter::once(&record.name))
        .cloned()
        .collect::<Vec<_>>()
        .join("::")
}

fn module_id(path: &str) -> String {
    format!("module:{path}")
}

fn symbol_key(path: &str, definition: &Definition) -> (String, usize, String) {
    (
        path.to_owned(),
        definition.start_byte,
        definition.name.clone(),
    )
}
fn resolved_key(symbol: &SymbolId) -> (String, usize, String) {
    (symbol.file.clone(), symbol.start_byte, symbol.name.clone())
}

fn deduplicate(relationships: Vec<Relationship>) -> Vec<Relationship> {
    let mut edges = BTreeMap::<_, Relationship>::new();
    for relationship in relationships {
        let key = (
            relationship.source.clone(),
            relationship.target.clone(),
            relationship.kind,
            relationship.contract,
            relationship.ordered,
            relationship.resolution,
        );
        match edges.entry(key) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(relationship);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                entry.get_mut().evidence.extend(relationship.evidence)
            }
        }
    }
    edges
        .into_values()
        .map(|mut edge| {
            edge.evidence.sort();
            edge.evidence.dedup();
            edge
        })
        .collect()
}
