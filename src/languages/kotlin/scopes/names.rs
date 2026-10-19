use super::*;

pub(super) fn scope_name(
    node: Node<'_>,
    index: &ScopeIndex<'_>,
    query_functions: &[FunctionScope],
) -> String {
    let source = index.source();
    match node.kind() {
        "function_declaration" => declaration_name(node, source, "function"),
        "enum_entry" => declaration_name(node, source, "enum_entry"),
        "getter" => "get".to_owned(),
        "setter" => "set".to_owned(),
        "anonymous_initializer" => "init".to_owned(),
        "lambda_literal" | "anonymous_function" => bound_lambda_name(node, source)
            .or_else(|| query_name(query_functions, node))
            .unwrap_or_else(|| format!("anonymous@{}", node.start_position().row + 1)),
        "primary_constructor" => primary_constructor_name(node, source, index.recovered_types()),
        "secondary_constructor" => format!(
            "{}.constructor",
            enclosing_type_name(node, source, index.recovered_types())
        ),
        "property_declaration" => property_name(node, source),
        "destructuring_declaration" => format!("destructuring@{}", node.start_position().row + 1),
        _ => unreachable!("only captured executable nodes have scope names"),
    }
}
pub(super) fn bound_lambda_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    let mut parent = node.parent()?;
    if parent.kind() == "annotated_lambda" {
        parent = parent.parent()?;
    }
    (parent.kind() == "property_declaration"
        && bound_function_value(parent).is_some_and(|value| value.id() == node.id()))
    .then(|| property_name(parent, source))
}
pub(super) fn declaration_name(node: Node<'_>, source: &[u8], fallback: &str) -> String {
    let candidate = match node.kind() {
        "enum_entry" | "function_declaration" => direct_named_child(node, "simple_identifier"),
        _ => node.child_by_field_name("name"),
    };
    candidate
        .map(|name| normalized_identifier(name, source))
        .unwrap_or_else(|| format!("{fallback}@{}", node.start_position().row + 1))
}
pub(super) fn property_name(node: Node<'_>, source: &[u8]) -> String {
    direct_named_child(node, "variable_declaration")
        .and_then(|declaration| direct_named_child(declaration, "simple_identifier"))
        .map(|name| normalized_identifier(name, source))
        .unwrap_or_else(|| format!("property@{}", node.start_position().row + 1))
}
pub(super) fn primary_constructor_name(
    node: Node<'_>,
    source: &[u8],
    recovered_types: &[syntax::RecoveredType],
) -> String {
    node.parent()
        .filter(|parent| parent.kind() == "class_declaration")
        .and_then(|class| direct_named_child(class, "type_identifier"))
        .map(|name| normalized_identifier(name, source))
        .or_else(|| {
            syntax::recovered_type_for(node, recovered_types)
                .and_then(|recovered| recovered.name_range.as_ref())
                .map(|range| normalized_source_identifier(range, source))
        })
        .unwrap_or_else(|| format!("constructor@{}", node.start_position().row + 1))
}
pub(super) fn has_bodyless_api_primary_constructor(
    node: Node<'_>,
    recovered_types: &[syntax::RecoveredType],
) -> bool {
    if node
        .parent()
        .filter(|parent| matches!(parent.kind(), "class_declaration" | "object_declaration"))
        .and_then(|declaration| direct_named_child(declaration, "modifiers"))
        .is_some_and(|modifiers| {
            has_descendant_kind(modifiers, "expect") || has_descendant_kind(modifiers, "external")
        })
    {
        return true;
    }
    let Some(recovered) = syntax::recovered_type_for(node, recovered_types) else {
        return false;
    };
    std::iter::successors(node.parent(), |ancestor| ancestor.parent())
        .find(|ancestor| {
            ancestor.kind() == "ERROR" && ancestor.byte_range() == recovered.error_range
        })
        .and_then(|error| {
            (0..error.child_count())
                .filter_map(|index| error.child(index))
                .find(|child| {
                    child.kind() == "modifiers" && child.end_byte() <= recovered.keyword_range.start
                })
        })
        .is_some_and(|modifiers| {
            has_descendant_kind(modifiers, "expect") || has_descendant_kind(modifiers, "external")
        })
}
pub(super) fn has_descendant_kind(node: Node<'_>, kind: &str) -> bool {
    super::super::all_nodes(node)
        .iter()
        .any(|candidate| candidate.kind() == kind)
}

pub(super) fn enclosing_type_name(
    node: Node<'_>,
    source: &[u8],
    recovered_types: &[syntax::RecoveredType],
) -> String {
    if let Some(parent) = super::super::ancestors(node)
        .skip(1)
        .find(|parent| matches!(parent.kind(), "class_declaration" | "object_declaration"))
    {
        return direct_named_child(parent, "type_identifier")
            .map(|name| normalized_identifier(name, source))
            .unwrap_or_else(|| "anonymous".to_owned());
    }
    syntax::recovered_type_for(node, recovered_types)
        .and_then(|recovered| recovered.name_range.as_ref())
        .map(|range| normalized_source_identifier(range, source))
        .unwrap_or_else(|| "constructor".to_owned())
}
pub(super) fn recovered_error(node: Node<'_>, recovered_types: &[syntax::RecoveredType]) -> bool {
    recovered_types
        .iter()
        .any(|recovered| recovered.error_range == node.byte_range())
}

pub(super) fn query_name(functions: &[FunctionScope], node: Node<'_>) -> Option<String> {
    functions
        .iter()
        .find(|function| {
            function.range.start == node.start_byte() && function.range.end == node.end_byte()
        })
        .map(|function| function.name.clone())
}
pub(super) fn scope(name: String, start: usize, end: usize, source: &[u8]) -> FunctionScope {
    FunctionScope {
        name,
        range: start..end,
        line: source[..start.min(source.len())]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count()
            + 1,
    }
}
