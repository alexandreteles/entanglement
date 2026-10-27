use super::*;

pub(super) fn method_signatures(
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
                        super::super::member_id(fact, store, record),
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
