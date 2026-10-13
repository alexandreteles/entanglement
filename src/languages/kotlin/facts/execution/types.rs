use super::*;

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
            if let Some(range) = crate::languages::kotlin::scopes::property_initializer_range(
                member,
                scope_index.contexts(),
            ) {
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
    for node in all_nodes(scope_index.root()) {
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
    match member.kind() {
        "secondary_constructor" => {
            if let Some(range) = scope_index.metric_scope_range_for(member) {
                execution.constructors.push(source_range(range));
            }
        }
        "property_declaration" => {
            if let Some(range) = crate::languages::kotlin::scopes::property_initializer_range(
                member,
                scope_index.contexts(),
            ) {
                execution.initializers.push(source_range(range));
            }
        }
        "anonymous_initializer" | "enum_entry" => {
            if let Some(range) = scope_index.metric_scope_range_for(member) {
                execution.initializers.push(source_range(range));
            }
        }
        _ => {}
    }
}
