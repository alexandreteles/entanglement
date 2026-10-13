use tree_sitter::Node;

use crate::metrics::cyclomatic::FunctionScope;

use super::{nodes::*, syntax};

pub(super) fn executable_scope_name(
    node: Node<'_>,
    source: &[u8],
    query_functions: &[FunctionScope],
    recovered_types: &[syntax::RecoveredType],
) -> Option<String> {
    callable_scope_name(node, source)
        .or_else(|| lambda_scope_name(node, source, query_functions))
        .or_else(|| constructor_scope_name(node, source, recovered_types))
        .or_else(|| initializer_scope_name(node, source))
}
pub(super) fn callable_scope_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    match node.kind() {
        "function_declaration" if has_named_child(node, "function_body") => {
            Some(declaration_name(node, source, "function"))
        }
        "getter" if has_named_child(node, "function_body") => Some("get".to_owned()),
        "setter" if has_named_child(node, "function_body") => Some("set".to_owned()),
        _ => None,
    }
}
pub(super) fn lambda_scope_name(
    node: Node<'_>,
    source: &[u8],
    query_functions: &[FunctionScope],
) -> Option<String> {
    match node.kind() {
        "lambda_literal" => Some(
            bound_lambda_name(node, source)
                .or_else(|| query_name(query_functions, node))
                .unwrap_or_else(|| format!("anonymous@{}", node.start_position().row + 1)),
        ),
        "anonymous_function" if has_named_child(node, "function_body") => Some(
            bound_lambda_name(node, source)
                .or_else(|| query_name(query_functions, node))
                .unwrap_or_else(|| format!("anonymous@{}", node.start_position().row + 1)),
        ),
        _ => None,
    }
}
pub(super) fn constructor_scope_name(
    node: Node<'_>,
    source: &[u8],
    recovered_types: &[syntax::RecoveredType],
) -> Option<String> {
    match node.kind() {
        "primary_constructor" if !has_bodyless_api_primary_constructor(node, recovered_types) => {
            Some(primary_constructor_name(node, source, recovered_types))
        }
        "secondary_constructor" if has_direct_child(node, "{") => Some(format!(
            "{}.constructor",
            enclosing_type_name(node, source, recovered_types)
        )),
        _ => None,
    }
}
pub(super) fn initializer_scope_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    match node.kind() {
        "property_declaration"
            if has_property_initializer(node) && bound_function_value(node).is_none() =>
        {
            Some(property_name(node, source))
        }
        "destructuring_declaration" if has_direct_child(node, "=") => {
            Some(format!("destructuring@{}", node.start_position().row + 1))
        }
        "anonymous_initializer" => Some("init".to_owned()),
        "enum_entry" if has_named_child(node, "value_arguments") => {
            Some(declaration_name(node, source, "enum_entry"))
        }
        _ => None,
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
        "enum_entry" => direct_named_child(node, "simple_identifier"),
        "function_declaration" => direct_named_child(node, "simple_identifier"),
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
    let mut pending = vec![node];
    while let Some(candidate) = pending.pop() {
        if candidate.kind() == kind {
            return true;
        }
        for index in (0..candidate.child_count()).rev() {
            if let Some(child) = candidate.child(index) {
                pending.push(child);
            }
        }
    }
    false
}
pub(super) fn enclosing_type_name(
    node: Node<'_>,
    source: &[u8],
    recovered_types: &[syntax::RecoveredType],
) -> String {
    let mut current = node.parent();
    while let Some(parent) = current {
        if matches!(parent.kind(), "class_declaration" | "object_declaration") {
            return direct_named_child(parent, "type_identifier")
                .map(|name| normalized_identifier(name, source))
                .unwrap_or_else(|| "anonymous".to_owned());
        }
        current = parent.parent();
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
