mod adapters;
mod go;
mod python;

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

impl Analysis<'_> {
    fn declaration(&self, unit: &str, declaration: &Declaration<'_>) -> EffectiveDeclaration {
        if self.ambiguous.contains(unit) {
            return EffectiveDeclaration {
                unit: unit.to_owned(),
                evidence: declaration_evidence(unit, self.facts),
                status: CoverageStatus::Unavailable,
                members: Vec::new(),
                types: Vec::new(),
                obligations: Vec::new(),
                conflicts: vec!["identity_ambiguous".to_owned()],
                satisfies: Vec::new(),
            };
        }
        let policy = adapters::Policy::for_declaration(
            declaration
                .store
                .semantic
                .structure_language
                .as_deref()
                .unwrap_or(""),
            &declaration.record.kind,
        );
        let mut obligations = self
            .snapshot
            .relationships
            .iter()
            .filter(|edge| {
                edge.source == unit
                    && matches!(
                        edge.kind,
                        RelationshipKind::Inherit
                            | RelationshipKind::Compose
                            | RelationshipKind::Implement
                    )
                    && edge.resolution != ResolutionStatus::Resolved
            })
            .map(|edge| format!("unresolved:{}", edge.target))
            .collect::<Vec<_>>();
        let paths = provenance(unit, &self.parents);
        obligations.extend(
            paths
                .keys()
                .filter(|origin| self.ambiguous.contains(*origin))
                .map(|origin| format!("ambiguous_origin:{origin}")),
        );
        let mut conflicts = Vec::new();
        let order = policy
            .order(unit, &self.parents, &paths)
            .unwrap_or_else(|error| {
                conflicts.push(error);
                vec![unit.to_owned()]
            });
        let selected = self
            .selections
            .iter()
            .filter(|selection| {
                symbol_unit(&selection.owner, &self.declarations).as_deref() == Some(unit)
            })
            .map(|selection| (selection.name.clone(), selection.origins.as_slice()))
            .collect::<BTreeMap<_, _>>();
        let candidates = candidates(&order, &paths, &self.direct);
        let mut members = self.select_members(unit, candidates, &selected, &policy, &order);
        self.add_aliases(unit, &selected, &mut members);
        obligations.extend(self.apply_selection(unit, &mut members));
        conflicts.extend(policy.conflicts(&mut members));
        obligations.extend(policy.finish(&mut members, self.snapshot));
        let mut types: Vec<TypeStructure> = declaration
            .record
            .types
            .iter()
            .map(|written| TypeStructure {
                written: written.clone(),
                expanded: expand_alias(
                    written,
                    unit,
                    &self.declarations,
                    self.snapshot,
                    &mut BTreeSet::new(),
                ),
                origin: unit.to_owned(),
            })
            .chain(
                declaration
                    .record
                    .alias
                    .iter()
                    .map(|written| TypeStructure {
                        written: written.clone(),
                        expanded: None,
                        origin: unit.to_owned(),
                    }),
            )
            .chain(
                members
                    .iter()
                    .flat_map(|member| member_types(&member.origin, self.facts)),
            )
            .collect();
        for structure in &mut types {
            if structure.expanded.is_some() {
                continue;
            }
            let source = self
                .direct
                .iter()
                .find(|(_, members)| {
                    members
                        .iter()
                        .any(|member| member.origin == structure.origin)
                })
                .map_or(unit, |(source, _)| source);
            structure.expanded = expand_alias(
                &structure.written,
                source,
                &self.declarations,
                self.snapshot,
                &mut BTreeSet::new(),
            );
        }
        members
            .sort_by(|left, right| (&left.name, &left.origin).cmp(&(&right.name, &right.origin)));
        obligations.sort();
        obligations.dedup();
        conflicts.sort();
        conflicts.dedup();
        EffectiveDeclaration {
            unit: unit.to_owned(),
            evidence: self.evidence(unit),
            status: if declaration.store.semantic.structure_errors {
                CoverageStatus::Unavailable
            } else {
                CoverageStatus::Partial
            },
            members,
            types,
            obligations,
            conflicts,
            satisfies: Vec::new(),
        }
    }

    fn select_members(
        &self,
        unit: &str,
        candidates: BTreeMap<(String, Option<String>), Vec<EffectiveMember>>,
        selected: &BTreeMap<String, &[SymbolId]>,
        policy: &adapters::Policy,
        order: &[String],
    ) -> Vec<EffectiveMember> {
        let own = self.direct.get(unit).map(Vec::as_slice).unwrap_or_default();
        let mut members = Vec::new();
        for ((name, _), mut choices) in candidates {
            choices.sort_by(|left, right| {
                (left.provenance.len(), &left.origin).cmp(&(right.provenance.len(), &right.origin))
            });
            choices.dedup_by(|left, right| left.origin == right.origin);
            let contracts = policy.inherited_contracts(&choices);
            let chosen = if let Some(origins) = selected.get(&name) {
                let ids = origins
                    .iter()
                    .filter_map(|symbol| symbol_member(symbol, self.facts))
                    .collect::<BTreeSet<_>>();
                choices
                    .into_iter()
                    .filter(|choice| ids.contains(&choice.origin))
                    .collect::<Vec<_>>()
            } else {
                policy.choose(choices, own, &name, order)
            };
            members.extend(chosen.into_iter().map(|mut member| {
                member.contracts.extend(contracts.iter().cloned());
                member.contracts.sort();
                member.contracts.dedup();
                member
            }));
        }
        members
    }

    fn evidence(&self, unit: &str) -> Vec<Evidence> {
        let mut evidence = declaration_evidence(unit, self.facts);
        let mut visited = BTreeSet::from([unit.to_owned()]);
        let mut pending = vec![unit.to_owned()];
        while let Some(source) = pending.pop() {
            for edge in self.snapshot.relationships.iter().filter(|edge| {
                edge.source == source
                    && edge.kind == RelationshipKind::Type
                    && edge.resolution == ResolutionStatus::Resolved
            }) {
                if self
                    .declarations
                    .get(&edge.target)
                    .is_some_and(|target| target.record.alias.is_some())
                    && visited.insert(edge.target.clone())
                {
                    evidence.extend(declaration_evidence(&edge.target, self.facts));
                    pending.push(edge.target.clone());
                }
            }
        }
        evidence.sort();
        evidence.dedup();
        evidence
    }

    fn apply_selection(&self, unit: &str, members: &mut [EffectiveMember]) -> Vec<String> {
        let mut obligations = Vec::new();
        for selection in self.selections.iter().filter(|selection| {
            symbol_unit(&selection.owner, &self.declarations).as_deref() == Some(unit)
        }) {
            obligations.extend(selection.obligations.iter().cloned());
            for (symbol, visibility) in &selection.visibility {
                let Some(origin) = symbol_member(symbol, self.facts) else {
                    continue;
                };
                for member in members
                    .iter_mut()
                    .filter(|member| member.name == selection.name && member.origin == origin)
                {
                    member.visibility = visibility.clone();
                    if !selection.obligations.is_empty()
                        && member
                            .evidence
                            .iter()
                            .any(|evidence| evidence.detail == "property")
                    {
                        member.body = None;
                        member.selection = Some("candidate".to_owned());
                    }
                }
            }
        }
        obligations
    }

    fn add_aliases(
        &self,
        unit: &str,
        selected: &BTreeMap<String, &[SymbolId]>,
        members: &mut Vec<EffectiveMember>,
    ) {
        for (name, origins) in selected {
            if members.iter().any(|member| member.name == *name) {
                continue;
            }
            for symbol in *origins {
                if let Some(origin) = symbol_member(symbol, self.facts)
                    && let Some(original) = self
                        .direct
                        .values()
                        .flatten()
                        .find(|member| member.origin == origin)
                {
                    let mut member = original.clone();
                    member.name = name.clone();
                    member.provenance = vec![unit.to_owned(), origin];
                    members.push(member);
                }
            }
        }
    }
}

fn declaration_evidence(unit: &str, facts: &[FileFacts]) -> Vec<Evidence> {
    facts
        .iter()
        .flat_map(|fact| {
            fact.contexts.iter().flat_map(move |store| {
                store
                    .semantic
                    .structure
                    .iter()
                    .filter(move |record| {
                        record.is_type && extract::type_id(fact, store, record) == unit
                    })
                    .map(move |record| Evidence {
                        path: fact.analysis.path.clone(),
                        start: record.start,
                        end: record.end,
                        owner: unit.to_owned(),
                        detail: record.kind.clone(),
                    })
            })
        })
        .collect()
}

fn candidates(
    order: &[String],
    paths: &BTreeMap<String, Vec<String>>,
    direct: &BTreeMap<String, Vec<EffectiveMember>>,
) -> BTreeMap<(String, Option<String>), Vec<EffectiveMember>> {
    let mut candidates = BTreeMap::<(String, Option<String>), Vec<EffectiveMember>>::new();
    for origin in order {
        for member in direct.get(origin).into_iter().flatten() {
            let mut member = member.clone();
            member.provenance = paths
                .get(origin)
                .cloned()
                .unwrap_or_else(|| vec![origin.clone()]);
            candidates
                .entry((member.name.clone(), member.member_kind.clone()))
                .or_default()
                .push(member);
        }
    }
    candidates
}

fn direct_members(facts: &[FileFacts]) -> BTreeMap<String, Vec<EffectiveMember>> {
    let mut members = BTreeMap::<String, Vec<EffectiveMember>>::new();
    for fact in facts {
        for store in &fact.contexts {
            for record in store.semantic.structure.iter().filter(|record| {
                !record.is_type && record.kind != "impl" && !record.owner.is_empty()
            }) {
                let Some(unit) = extract::type_owner(facts, fact, store, record.start) else {
                    continue;
                };
                members
                    .entry(unit.clone())
                    .or_default()
                    .push(direct_member(fact, store, record, unit));
            }
        }
    }
    members
}

fn direct_member(
    fact: &FileFacts,
    store: &SemanticStore,
    record: &SyntaxDeclaration,
    unit: String,
) -> EffectiveMember {
    let origin = member_id(fact, store, record);
    let visibility = store
        .definitions
        .iter()
        .find(|definition| {
            definition.name == record.name
                && record.start <= definition.start_byte
                && definition.start_byte < record.end
        })
        .and_then(|definition| definition.declaration.as_ref())
        .map_or_else(
            || {
                if record.public {
                    "public".to_owned()
                } else {
                    "unknown".to_owned()
                }
            },
            |declaration| format!("{:?}", declaration.visibility).to_lowercase(),
        );
    EffectiveMember {
        name: record.name.clone(),
        required: record.required,
        optional: record.optional,
        receiver: record.receiver.clone(),
        member_kind: record.member_kind.clone(),
        contracts: vec![origin.clone()],
        visibility,
        origin: origin.clone(),
        body: (record.body_start.is_some() && !record.required).then_some(origin),
        selection: None,
        provenance: vec![unit],
        evidence: vec![Evidence {
            path: fact.analysis.path.clone(),
            start: record.start,
            end: record.end,
            owner: extract::qualified(record),
            detail: record.kind.clone(),
        }],
    }
}

fn parent_map(snapshot: &StructureSnapshot) -> BTreeMap<String, Vec<String>> {
    let mut edges = snapshot
        .relationships
        .iter()
        .filter(|edge| {
            edge.resolution == ResolutionStatus::Resolved
                && matches!(
                    edge.kind,
                    RelationshipKind::Inherit
                        | RelationshipKind::Compose
                        | RelationshipKind::Implement
                )
                && edge.source.starts_with("type:")
                && edge.target.starts_with("type:")
        })
        .collect::<Vec<_>>();
    edges.sort_by_key(|edge| {
        edge.evidence
            .iter()
            .filter(|evidence| !matches!(evidence.detail.as_str(), "relation" | "embed_ptr"))
            .map(|evidence| evidence.start)
            .min()
            .unwrap_or(0)
    });
    let mut parents = BTreeMap::<String, Vec<String>>::new();
    for edge in edges {
        let list = parents.entry(edge.source.clone()).or_default();
        if !list.contains(&edge.target) {
            list.push(edge.target.clone());
        }
    }
    parents
}

fn provenance(
    unit: &str,
    parents: &BTreeMap<String, Vec<String>>,
) -> BTreeMap<String, Vec<String>> {
    let mut paths = BTreeMap::from([(unit.to_owned(), vec![unit.to_owned()])]);
    let mut pending = VecDeque::from([unit.to_owned()]);
    while let Some(current) = pending.pop_front() {
        for parent in parents.get(&current).into_iter().flatten() {
            if paths.contains_key(parent) {
                continue;
            }
            let mut path = paths[&current].clone();
            path.push(parent.clone());
            paths.insert(parent.clone(), path);
            pending.push_back(parent.clone());
        }
    }
    paths
}

fn member_id(fact: &FileFacts, store: &SemanticStore, record: &SyntaxDeclaration) -> String {
    let origin = format!(
        "{}#{}:{}",
        fact.analysis.path,
        store
            .semantic
            .structure_language
            .as_deref()
            .unwrap_or("unknown"),
        extract::qualified(record)
    );
    let origin = record
        .member_kind
        .as_ref()
        .map_or(origin.clone(), |kind| format!("{origin}#{kind}"));
    record
        .impl_trait
        .as_ref()
        .map_or(origin.clone(), |trait_name| {
            format!("{origin}@{trait_name}")
        })
}

fn symbol_member(symbol: &SymbolId, facts: &[FileFacts]) -> Option<String> {
    facts
        .iter()
        .find(|fact| fact.analysis.path == symbol.file)
        .and_then(|fact| {
            fact.contexts.iter().find_map(|store| {
                store
                    .semantic
                    .structure
                    .iter()
                    .find(|record| {
                        record.start <= symbol.start_byte
                            && symbol.start_byte < record.end
                            && record.name == symbol.name
                    })
                    .map(|record| member_id(fact, store, record))
            })
        })
}

fn symbol_unit(
    symbol: &SymbolId,
    declarations: &BTreeMap<String, Declaration<'_>>,
) -> Option<String> {
    declarations
        .iter()
        .find(|(_, declaration)| {
            declaration.fact.analysis.path == symbol.file
                && declaration.record.start <= symbol.start_byte
                && symbol.start_byte < declaration.record.end
                && declaration.record.name == symbol.name
        })
        .map(|(unit, _)| unit.clone())
}

fn member_types(origin: &str, facts: &[FileFacts]) -> Vec<TypeStructure> {
    facts
        .iter()
        .flat_map(|fact| {
            fact.contexts.iter().flat_map(move |store| {
                store
                    .semantic
                    .structure
                    .iter()
                    .filter(move |record| record.public && member_id(fact, store, record) == origin)
                    .flat_map(move |record| {
                        record.types.iter().map(move |written| TypeStructure {
                            written: written.clone(),
                            expanded: None,
                            origin: origin.to_owned(),
                        })
                    })
            })
        })
        .collect()
}

fn expand_alias(
    written: &super::types::TypeExpression,
    unit: &str,
    declarations: &BTreeMap<String, Declaration<'_>>,
    snapshot: &StructureSnapshot,
    seen: &mut BTreeSet<String>,
) -> Option<super::types::TypeExpression> {
    let target = snapshot
        .relationships
        .iter()
        .filter(|edge| {
            edge.source == unit
                && edge.kind == RelationshipKind::Type
                && edge.resolution == ResolutionStatus::Resolved
        })
        .filter_map(|edge| {
            declarations
                .get(&edge.target)
                .map(|declaration| (&edge.target, declaration))
        })
        .find(|(_, declaration)| {
            declaration.record.name == written.text && declaration.record.alias.is_some()
        });
    if let Some((target, declaration)) = target {
        if !seen.insert(target.clone()) {
            return None;
        }
        let alias = declaration.record.alias.as_ref()?;
        return Some(
            expand_alias(alias, target, declarations, snapshot, seen)
                .unwrap_or_else(|| alias.clone()),
        );
    }
    let mut expanded = written.clone();
    let mut changed = false;
    for (original, child) in written.children.iter().zip(&mut expanded.children) {
        if let Some(replacement) =
            expand_alias(original, unit, declarations, snapshot, &mut seen.clone())
        {
            expanded.text = expanded.text.replacen(&original.text, &replacement.text, 1);
            *child = replacement;
            changed = true;
        }
    }
    changed.then_some(expanded)
}

/// Compare established requirements, type structure, and selected bodies.
/// Reduced coverage produces a coverage event instead of a claimed removal.
pub(crate) fn compare(
    before: &[EffectiveDeclaration],
    after: &[EffectiveDeclaration],
) -> Vec<StructuralEvent> {
    let before = before
        .iter()
        .map(|declaration| (&declaration.unit, declaration))
        .collect::<BTreeMap<_, _>>();
    let current_units = after
        .iter()
        .map(|declaration| &declaration.unit)
        .collect::<BTreeSet<_>>();
    let mut events: Vec<_> = after
        .iter()
        .filter_map(|current| {
            let previous = before.get(&current.unit)?;
            if previous.status == CoverageStatus::Unavailable
                || current.status == CoverageStatus::Unavailable
            {
                return (previous.status != current.status).then(|| {
                    effective_event(
                        "effective_coverage",
                        previous,
                        current,
                        vec![format!("{:?}", previous.status).to_lowercase()],
                        vec![format!("{:?}", current.status).to_lowercase()],
                    )
                });
            }
            (semantic_signature(previous) != semantic_signature(current)).then(|| {
                effective_event(
                    "effective",
                    previous,
                    current,
                    effective_values(previous),
                    effective_values(current),
                )
            })
        })
        .collect();
    events.extend(
        after
            .iter()
            .filter(|declaration| !before.contains_key(&declaration.unit))
            .map(|declaration| presence_event("effective_added", declaration)),
    );
    events.extend(
        before
            .values()
            .filter(|declaration| !current_units.contains(&declaration.unit))
            .map(|declaration| presence_event("effective_removed", declaration)),
    );
    events.sort_by(|left, right| (&left.units, &left.kind).cmp(&(&right.units, &right.kind)));
    events
}

fn presence_event(kind: &str, declaration: &EffectiveDeclaration) -> StructuralEvent {
    let event_kind = if declaration.status == CoverageStatus::Unavailable {
        "effective_coverage"
    } else {
        kind
    };
    let values = if declaration.status == CoverageStatus::Unavailable {
        vec!["unavailable".to_owned()]
    } else {
        effective_values(declaration)
    };
    let mut event = effective_event(event_kind, declaration, declaration, values.clone(), values);
    if kind == "effective_added" {
        event.before.clear();
        event.before_path = None;
        event.before_paths.clear();
    } else {
        event.after.clear();
        event.after_path = None;
        event.after_paths.clear();
    }
    event
}

fn semantic_signature(declaration: &EffectiveDeclaration) -> Vec<String> {
    declaration
        .members
        .iter()
        .map(|member| {
            format!(
                "{:?}",
                (
                    &member.name,
                    member.required,
                    member.optional,
                    &member.origin,
                    &member.body,
                    &member.selection,
                    &member.receiver,
                    &member.member_kind,
                    &member.contracts,
                    &member.visibility
                )
            )
        })
        .chain(declaration.types.iter().map(|structure| {
            format!(
                "{:?}",
                (
                    structure.written.semantic_key(),
                    structure
                        .expanded
                        .as_ref()
                        .map(|expanded| expanded.semantic_key()),
                    &structure.origin
                )
            )
        }))
        .chain(declaration.satisfies.iter().map(|proof| {
            format!(
                "satisfies:{}:{}:{:?}:{}",
                proof.receiver,
                proof.contract,
                proof.status,
                proof.members.join(",")
            )
        }))
        .chain(declaration.obligations.iter().cloned())
        .chain(declaration.conflicts.iter().cloned())
        .collect()
}

fn effective_values(declaration: &EffectiveDeclaration) -> Vec<String> {
    declaration
        .members
        .iter()
        .map(|member| {
            format!(
                "{}:{}:{}:{}:{}:{}",
                member.name,
                if member.optional {
                    "optional"
                } else if member.required {
                    "req"
                } else {
                    "provided"
                },
                member.member_kind.as_deref().unwrap_or("member"),
                member.visibility,
                member.selection.as_deref().unwrap_or("known"),
                member.body.as_deref().unwrap_or("?")
            )
        })
        .chain(declaration.types.iter().map(|structure| {
            format!(
                "type:{}=>{}",
                structure.written.text,
                structure
                    .expanded
                    .as_ref()
                    .map_or("?", |expanded| expanded.text.as_str())
            )
        }))
        .chain(declaration.satisfies.iter().map(|proof| {
            format!(
                "satisfies:{}:{}:{:?}:{}",
                proof.receiver,
                proof.contract,
                proof.status,
                proof.members.join(",")
            )
        }))
        .chain(declaration.obligations.iter().cloned())
        .chain(declaration.conflicts.iter().cloned())
        .collect()
}

fn effective_event(
    kind: &str,
    previous: &EffectiveDeclaration,
    current: &EffectiveDeclaration,
    before: Vec<String>,
    after: Vec<String>,
) -> StructuralEvent {
    let before_paths = event_paths(previous);
    let after_paths = event_paths(current);
    StructuralEvent {
        kind: kind.to_owned(),
        granularity: UnitKind::Type,
        projection: Projection::Contract,
        units: vec![current.unit.clone()],
        before,
        after,
        changed: Vec::new(),
        before_path: before_paths.first().cloned(),
        after_path: after_paths.first().cloned(),
        before_paths,
        after_paths,
    }
}

fn event_paths(declaration: &EffectiveDeclaration) -> Vec<Witness> {
    let mut paths: Vec<_> = declaration
        .members
        .iter()
        .map(|member| {
            let mut evidence = member.evidence.clone();
            evidence.extend(declaration.evidence.clone());
            evidence.sort();
            evidence.dedup();
            Witness {
                units: member.provenance.clone(),
                evidence,
            }
        })
        .collect();
    if paths.is_empty() {
        paths.push(Witness {
            units: vec![declaration.unit.clone()],
            evidence: declaration.evidence.clone(),
        });
    }
    paths
        .sort_by(|left, right| (&left.units, &left.evidence).cmp(&(&right.units, &right.evidence)));
    paths.dedup();
    paths
}
