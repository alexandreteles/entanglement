use super::EffectiveDeclaration;
use crate::model::{FileFacts, Resolution, SemanticStore};
use crate::structure::{
    extract,
    model::{CoverageStatus, RelationshipKind, ResolutionStatus, StructureSnapshot},
    types::TypeExpression,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

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

fn receiver_proofs(
    declaration: &EffectiveDeclaration,
    contract: &EffectiveDeclaration,
    requirements: &BTreeMap<String, String>,
    signatures: &BTreeMap<String, String>,
    is_interface: bool,
) -> Vec<Satisfaction> {
    ["value", "pointer"]
        .into_iter()
        .filter(|receiver| !is_interface || *receiver == "value")
        .filter(|receiver| {
            satisfied(
                declaration,
                receiver,
                requirements,
                signatures,
                is_interface,
            )
        })
        .map(|receiver| Satisfaction {
            contract: contract.unit.clone(),
            receiver: receiver.to_owned(),
            status: CoverageStatus::Complete,
            members: requirements.keys().cloned().collect(),
        })
        .collect()
}

fn eligible(
    declaration: &EffectiveDeclaration,
    records: &BTreeMap<String, TypeRecord<'_>>,
    snapshot: &StructureSnapshot,
) -> bool {
    let Some(record) = records.get(&declaration.unit) else {
        return false;
    };
    declaration.status != CoverageStatus::Unavailable
        && declaration.conflicts.is_empty()
        && !declaration.obligations.iter().any(|obligation| {
            obligation.starts_with("unresolved:") || obligation.starts_with("promotion_ambiguous")
        })
        && !record.record.types.iter().any(|expression| {
            expression.form.contains("type_parameter")
                || expression.text.contains('~')
                || expression.text.contains('|')
        })
        && constraints_known(record, records, snapshot)
        && snapshot
            .relationships
            .iter()
            .filter(|edge| {
                edge.source == declaration.unit && edge.kind == RelationshipKind::Compose
            })
            .all(|edge| edge.resolution == ResolutionStatus::Resolved)
}

fn constraints_known(
    record: &TypeRecord<'_>,
    records: &BTreeMap<String, TypeRecord<'_>>,
    snapshot: &StructureSnapshot,
) -> bool {
    let mut visited = BTreeSet::new();
    let mut pending = vec![extract::type_id(record.fact, record.store, record.record)];
    while let Some(unit) = pending.pop() {
        if !visited.insert(unit.clone()) {
            continue;
        }
        let Some(record) = records.get(&unit) else {
            return false;
        };
        if record.store.semantic.structure_errors
            || record.record.types.iter().any(|expression| {
                expression.form.contains("type_parameter")
                    || expression.text.contains('~')
                    || expression.text.contains('|')
            })
        {
            return false;
        }
        if !direct_constraints_known(record, records, snapshot) {
            return false;
        }
        pending.extend(
            snapshot
                .relationships
                .iter()
                .filter(|edge| {
                    edge.source == unit
                        && edge.kind == RelationshipKind::Compose
                        && edge.resolution == ResolutionStatus::Resolved
                })
                .map(|edge| edge.target.clone()),
        );
    }
    true
}

fn direct_constraints_known(
    record: &TypeRecord<'_>,
    records: &BTreeMap<String, TypeRecord<'_>>,
    snapshot: &StructureSnapshot,
) -> bool {
    record
        .record
        .types
        .iter()
        .filter(|expression| expression.form == "type_elem")
        .all(|expression| {
            snapshot
                .relationships
                .iter()
                .filter(|edge| {
                    edge.source == extract::type_id(record.fact, record.store, record.record)
                        && edge.kind == RelationshipKind::Compose
                        && edge.resolution == ResolutionStatus::Resolved
                })
                .filter_map(|edge| records.get(&edge.target))
                .any(|target| {
                    target.record.kind == "interface" && target.record.name == expression.text
                })
        })
}

fn requirements(
    contract: &EffectiveDeclaration,
    signatures: &BTreeMap<String, String>,
) -> Option<BTreeMap<String, String>> {
    let mut requirements = BTreeMap::new();
    for member in contract.members.iter().filter(|member| member.required) {
        let signature = signatures.get(&member.origin)?;
        if requirements
            .insert(member.name.clone(), signature.clone())
            .is_some_and(|previous| previous != *signature)
        {
            return None;
        }
    }
    Some(requirements)
}

fn satisfied(
    declaration: &EffectiveDeclaration,
    receiver: &str,
    requirements: &BTreeMap<String, String>,
    signatures: &BTreeMap<String, String>,
    is_interface: bool,
) -> bool {
    requirements.iter().all(|(name, signature)| {
        let matches = declaration
            .members
            .iter()
            .filter(|member| {
                member.name == *name
                    && (is_interface || !member.required)
                    && (receiver == "pointer" || member.receiver.as_deref() != Some("pointer"))
            })
            .collect::<Vec<_>>();
        let unique = matches.len() == 1 || is_interface;
        unique
            && !matches.is_empty()
            && matches
                .iter()
                .all(|member| signatures.get(&member.origin) == Some(signature))
    })
}

fn method_signatures(
    facts: &[FileFacts],
    packages: &BTreeMap<(usize, usize), String>,
) -> BTreeMap<String, String> {
    facts
        .iter()
        .enumerate()
        .flat_map(|(file, fact)| {
            fact.contexts.iter().flat_map(move |store| {
                store.semantic.structure.iter().filter_map(move |record| {
                    let package = packages.get(&(file, store.semantic.context_id))?;
                    let signature = record.signature.as_ref()?;
                    if signature.generic {
                        return None;
                    }
                    let parameters = signature
                        .parameters
                        .iter()
                        .map(|expression| {
                            type_key(expression, fact, record, facts, &mut BTreeSet::new())
                        })
                        .collect::<Option<Vec<_>>>()?;
                    let results = signature
                        .results
                        .iter()
                        .map(|expression| {
                            type_key(expression, fact, record, facts, &mut BTreeSet::new())
                        })
                        .collect::<Option<Vec<_>>>()?;
                    Some((
                        super::member_id(fact, store, record),
                        format!(
                            "{:?}",
                            (
                                parameters,
                                results,
                                signature.variadic,
                                (!record.name.chars().next().is_some_and(char::is_uppercase))
                                    .then_some(package)
                            )
                        ),
                    ))
                })
            })
        })
        .collect()
}

fn type_key(
    expression: &TypeExpression,
    fact: &FileFacts,
    record: &extract::SyntaxDeclaration,
    facts: &[FileFacts],
    seen: &mut BTreeSet<String>,
) -> Option<String> {
    let resolved = fact.analysis.resolution.iter().find(|reference| {
        record.start <= reference.start_byte
            && reference.end_byte <= record.body_start.unwrap_or(record.end)
            && reference.path.join(".") == expression.text
    });
    if let Some(reference) =
        resolved.filter(|reference| !matches!(reference.resolution, Resolution::Unresolved))
    {
        return match &reference.resolution {
            Resolution::Exact(symbol) => named_type_key(symbol, facts, seen),
            Resolution::External => {
                builtin(&expression.text).map(|builtin| format!("builtin:{builtin}"))
            }
            _ => None,
        };
    }
    if let Some(builtin) = builtin(&expression.text) {
        let shadowed = facts
            .iter()
            .flat_map(|fact| &fact.contexts)
            .flat_map(|store| &store.semantic.structure)
            .any(|record| record.is_type && record.name == expression.text);
        return (!shadowed).then(|| format!("builtin:{builtin}"));
    }
    if matches!(
        expression.form.as_str(),
        "type_identifier"
            | "qualified_type"
            | "struct_type"
            | "interface_type"
            | "generic_type"
            | "type_instantiation_expression"
    ) {
        return None;
    }
    if expression.children.is_empty() {
        return Some(format!("{}:{}", expression.form, expression.text));
    }
    let children = expression
        .children
        .iter()
        .map(|child| type_key(child, fact, record, facts, &mut seen.clone()))
        .collect::<Option<Vec<_>>>()?;
    Some(format!("{}:{children:?}", expression.form))
}

fn named_type_key(
    symbol: &crate::model::SymbolId,
    facts: &[FileFacts],
    seen: &mut BTreeSet<String>,
) -> Option<String> {
    let identity = format!("{}:{:?}:{}", symbol.file, symbol.module, symbol.name);
    let target = facts
        .iter()
        .find(|fact| fact.analysis.path == symbol.file)?;
    let alias = target
        .contexts
        .iter()
        .flat_map(|store| &store.semantic.structure)
        .find(|record| {
            record.start <= symbol.start_byte
                && symbol.start_byte < record.end
                && record.name == symbol.name
                && record.alias.is_some()
        });
    let Some(alias) = alias else {
        return Some(identity);
    };
    if !seen.insert(identity) {
        return None;
    }
    type_key(alias.alias.as_ref()?, target, alias, facts, seen)
}

fn builtin(name: &str) -> Option<&str> {
    match name {
        "byte" => Some("uint8"),
        "rune" => Some("int32"),
        "any" => Some("interface{}"),
        "bool" | "string" | "int" | "int8" | "int16" | "int32" | "int64" | "uint" | "uint8"
        | "uint16" | "uint32" | "uint64" | "uintptr" | "float32" | "float64" | "complex64"
        | "complex128" | "error" => Some(name),
        _ => None,
    }
}
