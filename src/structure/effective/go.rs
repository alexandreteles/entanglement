use super::EffectiveDeclaration;
use crate::model::{FileFacts, Resolution, SemanticStore};
use crate::structure::{
    extract,
    model::{CoverageStatus, RelationshipKind, ResolutionStatus, StructureSnapshot},
    types::TypeExpression,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

mod eligibility;
mod proofs;
mod signatures;

use eligibility::eligible;
use proofs::{receiver_proofs, requirements};
use signatures::method_signatures;

/// A structural interface proof. It does not create a nominal dependency.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Satisfaction {
    pub contract: String,
    pub receiver: String,
    pub status: CoverageStatus,
    pub members: Vec<String>,
}

struct TypeRecord<'a> {
    file: usize,
    fact: &'a FileFacts,
    store: &'a SemanticStore,
    record: &'a extract::SyntaxDeclaration,
}

/// Prove method-set satisfaction only with complete, resolved signatures.
/// Generic substitutions and type-set constraints remain symbolic.
pub(super) fn analyze(
    facts: &[FileFacts],
    snapshot: &StructureSnapshot,
    declarations: &mut [EffectiveDeclaration],
) {
    let records = facts
        .iter()
        .enumerate()
        .flat_map(|(file, fact)| {
            fact.contexts
                .iter()
                .filter(|store| store.semantic.structure_language.as_deref() == Some("source.go"))
                .flat_map(move |store| {
                    store
                        .semantic
                        .structure
                        .iter()
                        .filter(|record| record.is_type)
                        .map(move |record| {
                            (
                                extract::type_id(fact, store, record),
                                TypeRecord {
                                    file,
                                    fact,
                                    store,
                                    record,
                                },
                            )
                        })
                })
        })
        .collect::<BTreeMap<_, _>>();
    if records.is_empty() {
        return;
    }
    let packages = crate::resolver::structural_go_contexts(facts);
    let signatures = method_signatures(facts, &packages);
    let contracts = declarations
        .iter()
        .filter(|declaration| {
            records.get(&declaration.unit).is_some_and(|record| {
                record.record.kind == "interface"
                    && packages.contains_key(&(record.file, record.store.semantic.context_id))
            }) && eligible(declaration, &records, snapshot)
        })
        .cloned()
        .collect::<Vec<_>>();
    for declaration in declarations
        .iter_mut()
        .filter(|declaration| eligible(declaration, &records, snapshot))
    {
        let record = &records[&declaration.unit];
        if !packages.contains_key(&(record.file, record.store.semantic.context_id)) {
            continue;
        }
        declaration.satisfies = contracts
            .iter()
            .filter(|contract| contract.unit != declaration.unit)
            .filter_map(|contract| {
                requirements(contract, &signatures).map(|requirements| (contract, requirements))
            })
            .flat_map(|(contract, requirements)| {
                receiver_proofs(
                    declaration,
                    contract,
                    &requirements,
                    &signatures,
                    record.record.kind == "interface",
                )
            })
            .collect();
    }
}
