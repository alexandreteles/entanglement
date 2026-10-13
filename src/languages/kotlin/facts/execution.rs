use super::*;

mod types;

pub(super) use types::recovered_type_execution;
use types::{constructor_execution, type_execution};

pub(super) fn is_executable_scope(kind: &str) -> bool {
    matches!(
        kind,
        "function_declaration"
            | "secondary_constructor"
            | "anonymous_function"
            | "lambda_literal"
            | "getter"
            | "setter"
            | "anonymous_initializer"
    )
}

pub(super) fn executable_body(node: Node<'_>) -> Option<Range<usize>> {
    if node.kind() == "lambda_literal" {
        return Some(range(node));
    }
    all_nodes(node)
        .into_iter()
        .filter(|child| child.start_byte() >= node.start_byte())
        .find(|child| child.kind() == "statements" || child.kind() == "function_body")
        .map(range)
        .or_else(|| Some(range(node)))
}

pub(super) fn class_body_scope(node: Node<'_>) -> Option<Range<usize>> {
    all_nodes(node)
        .into_iter()
        .find(|child| matches!(child.kind(), "class_body" | "enum_class_body"))
        .map(range)
}

pub(super) fn declaration_execution(
    node: Node<'_>,
    scope_index: &crate::languages::kotlin::scopes::ScopeIndex<'_>,
) -> Option<DeclarationExecutionFacts> {
    let execution = match node.kind() {
        "property_declaration" => property_execution(node, scope_index),
        "class_declaration" | "object_declaration" | "companion_object" => {
            type_execution(node, scope_index)
        }
        "secondary_constructor" => constructor_execution(node, scope_index),
        _ => DeclarationExecutionFacts::default(),
    };
    (!execution.constructors.is_empty()
        || !execution.initializers.is_empty()
        || execution.getter.is_some()
        || execution.setter.is_some()
        || execution.callable_value.is_some())
    .then_some(execution)
}

fn property_execution(
    property: Node<'_>,
    scope_index: &crate::languages::kotlin::scopes::ScopeIndex<'_>,
) -> DeclarationExecutionFacts {
    let mut execution = DeclarationExecutionFacts::default();
    for accessor in property_accessors(property) {
        let Some(range) = scope_index
            .metric_scope_range_for(accessor)
            .map(source_range)
        else {
            continue;
        };
        match accessor.kind() {
            "getter" => execution.getter = Some(range),
            "setter" => execution.setter = Some(range),
            _ => {}
        }
    }
    if let Some(range) = crate::languages::kotlin::scopes::property_initializer_range(
        property,
        scope_index.contexts(),
    ) {
        execution.initializers.push(source_range(range));
    }
    if is_callable_property(property, execution.getter.is_some(), scope_index.source()) {
        execution.callable_value = bound_callable_value(property)
            .and_then(|initializer| scope_index.metric_scope_range_for(initializer))
            .map(source_range);
    }
    execution
}

fn is_callable_property(property: Node<'_>, has_getter: bool, source: &[u8]) -> bool {
    let children = named_children(property);
    let immutable = children
        .iter()
        .find(|child| child.kind() == "binding_pattern_kind")
        .is_some_and(|kind| text(*kind, source) == "val");
    immutable
        && !has_getter
        && !children
            .iter()
            .any(|child| child.kind() == "property_delegate")
}

fn bound_callable_value(property: Node<'_>) -> Option<Node<'_>> {
    let declaration = named_children(property)
        .into_iter()
        .find(|child| child.kind() == "variable_declaration")?;
    let initializer = property_initializer(property, declaration)?;
    match initializer.kind() {
        "lambda_literal" | "anonymous_function" => Some(initializer),
        "annotated_lambda" => all_nodes(initializer)
            .into_iter()
            .find(|child| child.kind() == "lambda_literal"),
        _ => None,
    }
}

pub(super) fn property_accessors(property: Node<'_>) -> Vec<Node<'_>> {
    let mut accessors = named_children(property)
        .into_iter()
        .filter(|child| matches!(child.kind(), "getter" | "setter"))
        .collect::<Vec<_>>();
    let Some(parent) = property.parent() else {
        return accessors;
    };
    let siblings = named_children(parent);
    let Some(position) = siblings
        .iter()
        .position(|sibling| sibling.id() == property.id())
    else {
        return accessors;
    };
    for sibling in siblings.iter().skip(position + 1).copied() {
        if matches!(sibling.kind(), "line_comment" | "multiline_comment") {
            continue;
        }
        if matches!(sibling.kind(), "getter" | "setter") {
            if !accessors
                .iter()
                .any(|accessor| accessor.id() == sibling.id())
            {
                accessors.push(sibling);
            }
            continue;
        }
        break;
    }
    accessors
}

pub(super) fn source_range(range: Range<usize>) -> SourceRange {
    SourceRange {
        start_byte: range.start,
        end_byte: range.end,
    }
}
