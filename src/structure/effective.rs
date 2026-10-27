mod adapters;
mod assembly;
mod comparison;
mod comparison_values;
mod evidence;
mod go;
mod hierarchy;
mod members;
mod python;
mod selection;
mod types;

/// Compare established requirements, type structure, and selected bodies.
/// Reduced coverage produces a coverage event instead of a claimed removal.
pub(crate) use comparison::compare;
use evidence::declaration_evidence;
use hierarchy::{parent_map, provenance};
use members::{direct_members, member_id, symbol_member, symbol_unit};
use selection::candidates;
use types::{expand_alias, member_types};

use super::extract::{self, SyntaxDeclaration};
use super::model::*;
use super::types::TypeStructure;
use crate::model::{FileFacts, SemanticStore, SymbolId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// A member requirement and its established implementation origin.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveMember {
    pub name: String,
    pub required: bool,
    pub optional: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receiver: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_kind: Option<String>,
    pub origin: String,
    pub contracts: Vec<String>,
    pub visibility: String,
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<String>,
    pub provenance: Vec<String>,
    pub evidence: Vec<Evidence>,
}

/// Source-backed requirements and members for one declaration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveDeclaration {
    pub unit: String,
    pub evidence: Vec<Evidence>,
    pub status: CoverageStatus,
    pub members: Vec<EffectiveMember>,
    pub types: Vec<TypeStructure>,
    pub obligations: Vec<String>,
    pub conflicts: Vec<String>,
    pub satisfies: Vec<go::Satisfaction>,
}

struct Declaration<'a> {
    fact: &'a FileFacts,
    store: &'a SemanticStore,
    record: &'a SyntaxDeclaration,
}

struct Analysis<'a> {
    facts: &'a [FileFacts],
    snapshot: &'a StructureSnapshot,
    declarations: BTreeMap<String, Declaration<'a>>,
    direct: BTreeMap<String, Vec<EffectiveMember>>,
    parents: BTreeMap<String, Vec<String>>,
    selections: Vec<crate::resolver::StructuralMemberSelection>,
    ambiguous: BTreeSet<String>,
}

/// Collect source-backed candidates and apply the selected language policy.
/// Unknown runtime structure and external obligations remain explicit.
pub(crate) fn analyze(
    facts: &[FileFacts],
    snapshot: &StructureSnapshot,
    source_snapshot: &crate::snapshot::Snapshot,
) -> Vec<EffectiveDeclaration> {
    let records: Vec<_> = facts
        .iter()
        .flat_map(|fact| {
            fact.contexts.iter().flat_map(move |store| {
                store
                    .semantic
                    .structure
                    .iter()
                    .filter(|record| record.is_type)
                    .map(move |record| {
                        (
                            extract::type_id(fact, store, record),
                            Declaration {
                                fact,
                                store,
                                record,
                            },
                        )
                    })
            })
        })
        .collect();
    let mut identities = BTreeSet::new();
    let ambiguous = records
        .iter()
        .filter(|(unit, _)| !identities.insert(unit.clone()))
        .map(|(unit, _)| unit.clone())
        .collect::<BTreeSet<_>>();
    let analysis = Analysis {
        facts,
        snapshot,
        declarations: records.into_iter().collect(),
        direct: direct_members(facts)
            .into_iter()
            .filter(|(unit, _)| !ambiguous.contains(unit))
            .collect(),
        parents: parent_map(snapshot),
        selections: crate::resolver::structural_members(facts, source_snapshot),
        ambiguous,
    };
    let mut declarations = analysis
        .declarations
        .iter()
        .map(|(unit, declaration)| analysis.declaration(unit, declaration))
        .collect::<Vec<_>>();
    go::analyze(facts, snapshot, &mut declarations);
    declarations
}
