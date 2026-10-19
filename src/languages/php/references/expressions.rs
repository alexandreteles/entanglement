use super::*;

pub(in crate::languages::php) fn receiver_text(node: Node<'_>, source: &[u8]) -> String {
    let node = unwrap(node);
    if text(node, source) == "$this"
        && ancestors(node)
            .take_while(|node| node.kind() != "function_definition" && !is_class(*node))
            .any(|node| {
                matches!(
                    node.kind(),
                    "anonymous_function" | "arrow_function" | "method_declaration"
                ) && modifier(node, "static_modifier")
            })
    {
        return "@unbound:this".to_owned();
    }
    if node.kind() == "object_creation_expression"
        && let Some(kind) = children(node).find(|child| is_name(*child))
    {
        return format!("new {}", text(kind, source));
    }
    text(node, source).to_owned()
}

pub(super) fn pipe_target(node: Node<'_>) -> bool {
    let node = ancestors(node)
        .take_while(|parent| parent.kind() == "parenthesized_expression")
        .last()
        .unwrap_or(node);
    node.parent().is_some_and(|parent| {
        parent.kind() == "binary_expression"
            && parent
                .child_by_field_name("right")
                .is_some_and(|right| right.id() == node.id())
            && parent
                .child_by_field_name("operator")
                .is_some_and(|operator| operator.kind() == "|>")
    })
}

pub(super) fn constant_name(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    match parent.kind() {
        "binary_expression" => {
            parent
                .child_by_field_name("right")
                .is_none_or(|right| right.id() != node.id())
                || parent
                    .child_by_field_name("operator")
                    .is_none_or(|operator| !matches!(operator.kind(), "instanceof" | "|>"))
        }
        "argument" => parent
            .child_by_field_name("name")
            .is_none_or(|name| name.id() != node.id()),
        "return_statement"
        | "echo_statement"
        | "print_intrinsic"
        | "conditional_expression"
        | "parenthesized_expression"
        | "array_element_initializer"
        | "subscript_expression"
        | "unary_op_expression"
        | "expression_statement"
        | "yield_expression"
        | "match_condition_list"
        | "case_statement"
        | "sequence_expression"
        | "throw_expression"
        | "clone_expression"
        | "exit_statement" => true,
        "const_element" => parent
            .named_child(0)
            .is_none_or(|name| name.id() != node.id()),
        kind => {
            let field = match kind {
                "match_conditional_expression" | "match_default_expression" => "return_expression",
                "enum_case" | "static_variable_declaration" => "value",
                "assignment_expression" | "augmented_assignment_expression" => "right",
                "property_element" | "simple_parameter" | "property_promotion_parameter" => {
                    "default_value"
                }
                _ => return false,
            };
            parent
                .child_by_field_name(field)
                .is_some_and(|value| value.id() == node.id())
        }
    }
}

pub(super) fn receiver_reference(node: Node<'_>) -> Option<usize> {
    let node = unwrap(node);
    match node.kind() {
        "function_call_expression" => {
            node.child_by_field_name("function")
                .map(unwrap)
                .and_then(|target| {
                    if matches!(target.kind(), "anonymous_function" | "arrow_function") {
                        node.child_by_field_name("arguments")
                            .map(|arguments| arguments.start_byte())
                    } else {
                        Some(target.start_byte())
                    }
                })
        }
        "member_access_expression"
        | "nullsafe_member_access_expression"
        | "member_call_expression"
        | "nullsafe_member_call_expression"
        | "scoped_property_access_expression"
        | "scoped_call_expression" => node
            .child_by_field_name("name")
            .map(|target| target.start_byte()),
        _ => None,
    }
}

pub(super) fn access(node: Node<'_>) -> ReferenceAccess {
    let target = node
        .parent()
        .filter(|parent| {
            matches!(
                parent.kind(),
                "member_access_expression"
                    | "nullsafe_member_access_expression"
                    | "scoped_property_access_expression"
            )
        })
        .unwrap_or(node);
    match target.parent() {
        Some(parent) if parent.kind() == "update_expression" => ReferenceAccess::ReadWrite,
        Some(parent)
            if parent
                .child_by_field_name("left")
                .is_some_and(|left| left.id() == target.id()) =>
        {
            if parent.kind() == "augmented_assignment_expression" {
                ReferenceAccess::ReadWrite
            } else {
                ReferenceAccess::Write
            }
        }
        _ => ReferenceAccess::Read,
    }
}
