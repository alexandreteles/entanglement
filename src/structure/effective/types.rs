use super::*;

pub(super) fn member_types(origin: &str, facts: &[FileFacts]) -> Vec<TypeStructure> {
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

pub(super) fn expand_alias(
    written: &super::super::types::TypeExpression,
    unit: &str,
    declarations: &BTreeMap<String, Declaration<'_>>,
    snapshot: &StructureSnapshot,
    seen: &mut BTreeSet<String>,
) -> Option<super::super::types::TypeExpression> {
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
