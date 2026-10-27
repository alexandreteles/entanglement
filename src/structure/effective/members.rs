use super::*;

pub(super) fn direct_members(facts: &[FileFacts]) -> BTreeMap<String, Vec<EffectiveMember>> {
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

pub(super) fn member_id(
    fact: &FileFacts,
    store: &SemanticStore,
    record: &SyntaxDeclaration,
) -> String {
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

pub(super) fn symbol_member(symbol: &SymbolId, facts: &[FileFacts]) -> Option<String> {
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

pub(super) fn symbol_unit(
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
