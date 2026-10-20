use super::*;

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

pub(super) fn executable_body(node: Node<'_>) -> Range<usize> {
    if node.kind() == "lambda_literal" {
        return range(node);
    }
    all_nodes(node)
        .into_iter()
        .find(|child| child.kind() == "statements" || child.kind() == "function_body")
        .map(range)
        .unwrap_or_else(|| range(node))
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
    if let Some(range) = crate::languages::kotlin::scopes::property_initializer_range(property) {
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
    accessors.extend(
        siblings
            .into_iter()
            .skip(position + 1)
            .filter(|sibling| !matches!(sibling.kind(), "line_comment" | "multiline_comment"))
            .take_while(|sibling| matches!(sibling.kind(), "getter" | "setter")),
    );
    accessors
}

pub(super) fn source_range(range: Range<usize>) -> SourceRange {
    SourceRange {
        start_byte: range.start,
        end_byte: range.end,
    }
}

pub(super) fn type_execution(
    declaration: Node<'_>,
    scope_index: &crate::languages::kotlin::scopes::ScopeIndex<'_>,
) -> DeclarationExecutionFacts {
    let mut execution = DeclarationExecutionFacts::default();
    let children = named_children(declaration);
    if let Some(primary) = children
        .iter()
        .find(|child| child.kind() == "primary_constructor")
        && let Some(range) = scope_index.metric_scope_range_for(*primary)
    {
        execution.constructors.push(source_range(range));
    }
    if let Some(body) = children
        .iter()
        .find(|child| matches!(child.kind(), "class_body" | "enum_class_body"))
    {
        for member in named_children(*body) {
            collect_type_member_execution(member, scope_index, &mut execution);
        }
    }
    execution
}

pub(super) fn collect_type_member_execution(
    member: Node<'_>,
    scope_index: &crate::languages::kotlin::scopes::ScopeIndex<'_>,
    execution: &mut DeclarationExecutionFacts,
) {
    match member.kind() {
        "secondary_constructor" => {
            if let Some(range) = scope_index.metric_scope_range_for(member) {
                execution.constructors.push(source_range(range));
            }
        }
        "property_declaration" => {
            if let Some(range) =
                crate::languages::kotlin::scopes::property_initializer_range(member)
            {
                execution.initializers.push(source_range(range));
            }
        }
        "anonymous_initializer" => {
            if let Some(range) = scope_index.metric_scope_range_for(member) {
                execution.initializers.push(source_range(range));
            }
        }
        _ => {}
    }
}

pub(super) fn constructor_execution(
    constructor: Node<'_>,
    scope_index: &crate::languages::kotlin::scopes::ScopeIndex<'_>,
) -> DeclarationExecutionFacts {
    let mut execution = DeclarationExecutionFacts::default();
    if let Some(range) = scope_index.metric_scope_range_for(constructor) {
        execution.constructors.push(source_range(range));
    }
    execution
}

pub(in crate::languages::kotlin::facts) fn recovered_type_execution(
    recovered: &crate::languages::kotlin::syntax::RecoveredType,
    recovered_types: &[crate::languages::kotlin::syntax::RecoveredType],
    scope_index: &crate::languages::kotlin::scopes::ScopeIndex<'_>,
) -> Option<DeclarationExecutionFacts> {
    let mut execution = DeclarationExecutionFacts::default();
    for node in scope_index.nodes().iter().copied() {
        if is_recovered_primary_constructor(node, recovered, recovered_types) {
            if let Some(range) = scope_index.metric_scope_range_for(node) {
                execution.constructors.push(source_range(range));
            }
        } else if is_recovered_direct_member(node, recovered, recovered_types) {
            collect_recovered_member_execution(node, scope_index, &mut execution);
        }
    }
    (!execution.constructors.is_empty() || !execution.initializers.is_empty()).then_some(execution)
}

pub(super) fn is_recovered_primary_constructor(
    node: Node<'_>,
    recovered: &crate::languages::kotlin::syntax::RecoveredType,
    recovered_types: &[crate::languages::kotlin::syntax::RecoveredType],
) -> bool {
    node.kind() == "primary_constructor"
        && recovered.keyword_range.end <= node.start_byte()
        && node.end_byte() <= recovered.body_range.start
        && crate::languages::kotlin::syntax::recovered_type_for(node, recovered_types)
            .is_some_and(|owner| owner.declaration_range == recovered.declaration_range)
}

pub(super) fn is_recovered_direct_member(
    node: Node<'_>,
    recovered: &crate::languages::kotlin::syntax::RecoveredType,
    recovered_types: &[crate::languages::kotlin::syntax::RecoveredType],
) -> bool {
    recovered.body_range.start <= node.start_byte()
        && node.end_byte() <= recovered.body_range.end
        && crate::languages::kotlin::syntax::recovered_type_for(node, recovered_types)
            .is_some_and(|owner| owner.declaration_range == recovered.declaration_range)
        && !ancestors(node).any(|ancestor| {
            ancestor.start_byte() > recovered.declaration_range.start
                && matches!(
                    ancestor.kind(),
                    "class_declaration" | "object_declaration" | "companion_object"
                )
        })
}

pub(super) fn collect_recovered_member_execution(
    member: Node<'_>,
    scope_index: &crate::languages::kotlin::scopes::ScopeIndex<'_>,
    execution: &mut DeclarationExecutionFacts,
) {
    if member.kind() == "enum_entry" {
        if let Some(range) = scope_index.metric_scope_range_for(member) {
            execution.initializers.push(source_range(range));
        }
    } else {
        collect_type_member_execution(member, scope_index, execution);
    }
}
