mod adapter_members;
mod adapter_ownership;
mod adapter_relations;
mod adapter_visibility;
mod canonical;
mod facts;
mod fingerprint;
mod ownership;
mod references;
mod relationship_capture;
pub(crate) mod signatures;
pub(crate) use ownership::type_owner;
mod adapters;
mod capture;
use std::collections::BTreeMap;

use canonical::{canonical_modules, canonical_types};
use facts::{coverage, symbol_targets, units};
use relationship_capture::capture_relationships;

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
