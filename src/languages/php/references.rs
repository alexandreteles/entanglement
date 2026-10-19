//! Emit PHP references and keep invocation ownership separate from name lookup.

use tree_sitter::Node;

use crate::graph::{PhpFacts, PhpReference, PhpReferenceKind};
use crate::languages::CapturedTree;
use crate::model::{ModulePath, Reference, ReferenceAccess, ReferenceKind};

use super::syntax::*;

pub(super) fn capture(
    nodes: &[Node<'_>],
    source: &[u8],
    php: &mut PhpFacts,
    captured: &mut CapturedTree,
) {
    for node in nodes.iter().copied() {
        match node.kind() {
            "function_call_expression" => function(node, source, php, captured),
            "object_creation_expression" => construction(node, source, php, captured),
            "member_call_expression"
            | "nullsafe_member_call_expression"
            | "scoped_call_expression"
            | "member_access_expression"
            | "nullsafe_member_access_expression"
            | "scoped_property_access_expression" => member(node, source, php, captured),
            "class_constant_access_expression" => class_constant(node, source, php, captured),
            "named_type" | "base_clause" | "class_interface_clause" => {
                for name in children(node).into_iter().filter(|name| is_name(*name)) {
                    emit(
                        name,
                        text(name, source),
                        PhpReferenceKind::Class,
                        None,
                        false,
                        source,
                        php,
                        captured,
                    );
                }
            }
            "attribute" => {
                if let Some(name) = children(node).into_iter().find(|name| is_name(*name)) {
                    emit(
                        name,
                        text(name, source),
                        PhpReferenceKind::Class,
                        None,
                        false,
                        source,
                        php,
                        captured,
                    );
                }
            }
            "binary_expression" => binary(node, source, php, captured),
            "name" | "qualified_name" | "relative_name"
                if constant_name(node)
                    && !php
                        .references
                        .iter()
                        .any(|reference| reference.start_byte == node.start_byte()) =>
            {
                emit(
                    node,
                    text(node, source),
                    PhpReferenceKind::Constant,
                    None,
                    false,
                    source,
                    php,
                    captured,
                );
            }
            _ => {}
        }
    }
}

fn function(node: Node<'_>, source: &[u8], php: &mut PhpFacts, captured: &mut CapturedTree) {
    let Some(target) = node.child_by_field_name("function").map(unwrap) else {
        return;
    };
    if matches!(target.kind(), "anonymous_function" | "arrow_function") {
        if let Some(arguments) = node.child_by_field_name("arguments") {
            emit(
                arguments,
                &format!(
                    "anonymous@{}:{}",
                    target.start_position().row + 1,
                    target.start_byte()
                ),
                PhpReferenceKind::Function,
                None,
                !first_class_callable(node) || pipe_target(node),
                source,
                php,
                captured,
            );
        }
        return;
    }
    let name = literal(target, source)
        .map(|name| format!("\\{}", name.trim_start_matches('\\')))
        .unwrap_or_else(|| text(target, source).to_owned());
    let kind = if is_name(target) || literal(target, source).is_some() {
        PhpReferenceKind::Function
    } else {
        PhpReferenceKind::Variable
    };
    emit(
        target,
        &name,
        kind,
        None,
        !first_class_callable(node) || pipe_target(node),
        source,
        php,
        captured,
    );
}

fn construction(node: Node<'_>, source: &[u8], php: &mut PhpFacts, captured: &mut CapturedTree) {
    let Some(target) = children(node).into_iter().find(|child| {
        is_name(*child) || matches!(child.kind(), "variable_name" | "anonymous_class")
    }) else {
        return;
    };
    let name = if target.kind() == "anonymous_class" {
        format!(
            "\\{}",
            namespace(&php.namespaces, target)
                .name
                .iter()
                .map(String::as_str)
                .chain(std::iter::once(
                    format!("anonymous-class@{}", target.start_byte()).as_str()
                ))
                .collect::<Vec<_>>()
                .join("\\")
        )
    } else {
        text(target, source).to_owned()
    };
    emit(
        target,
        &name,
        PhpReferenceKind::Class,
        None,
        true,
        source,
        php,
        captured,
    );
}

fn member(node: Node<'_>, source: &[u8], php: &mut PhpFacts, captured: &mut CapturedTree) {
    let Some(name) = node.child_by_field_name("name") else {
        return;
    };
    let scoped = node.kind().starts_with("scoped_");
    let Some(receiver) = node.child_by_field_name(if scoped { "scope" } else { "object" }) else {
        return;
    };
    let call = node.kind().contains("call_expression");
    let kind = match (call, scoped) {
        (true, true) => PhpReferenceKind::StaticMethod,
        (true, false) => PhpReferenceKind::Method,
        (false, true) => PhpReferenceKind::StaticProperty,
        (false, false) => PhpReferenceKind::Property,
    };
    let receiver_text = receiver_text(receiver, source);
    emit(
        name,
        text(name, source),
        kind,
        Some(receiver_text),
        call && (!first_class_callable(node) || pipe_target(node)),
        source,
        php,
        captured,
    );
    if scoped && is_name(receiver) {
        emit(
            receiver,
            text(receiver, source),
            PhpReferenceKind::Class,
            None,
            false,
            source,
            php,
            captured,
        );
    }
}

pub(super) fn receiver_text(node: Node<'_>, source: &[u8]) -> String {
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
    if node.kind() == "object_creation_expression" {
        if let Some(kind) = children(node).into_iter().find(|child| is_name(*child)) {
            return format!("new {}", text(kind, source));
        }
    }
    text(node, source).to_owned()
}

fn class_constant(node: Node<'_>, source: &[u8], php: &mut PhpFacts, captured: &mut CapturedTree) {
    if node
        .parent()
        .is_some_and(|parent| matches!(parent.kind(), "use_as_clause" | "use_instead_of_clause"))
    {
        return;
    }
    let children = children(node);
    let (Some(receiver), Some(name)) = (children.first().copied(), children.last().copied()) else {
        return;
    };
    if name.id() == receiver.id() {
        return;
    }
    if !text(name, source).eq_ignore_ascii_case("class") {
        emit(
            name,
            text(name, source),
            PhpReferenceKind::ClassConstant,
            Some(receiver_text(receiver, source)),
            false,
            source,
            php,
            captured,
        );
    }
    if is_name(receiver) {
        emit(
            receiver,
            text(receiver, source),
            PhpReferenceKind::Class,
            None,
            false,
            source,
            php,
            captured,
        );
    }
}

fn binary(node: Node<'_>, source: &[u8], php: &mut PhpFacts, captured: &mut CapturedTree) {
    let Some(operator) = node.child_by_field_name("operator") else {
        return;
    };
    let Some(right) = node.child_by_field_name("right").map(unwrap) else {
        return;
    };
    match text(operator, source) {
        "|>" if matches!(right.kind(), "anonymous_function" | "arrow_function") => emit(
            operator,
            &format!(
                "anonymous@{}:{}",
                right.start_position().row + 1,
                right.start_byte()
            ),
            PhpReferenceKind::Function,
            None,
            true,
            source,
            php,
            captured,
        ),
        "instanceof" if is_name(right) => emit(
            right,
            text(right, source),
            PhpReferenceKind::Class,
            None,
            false,
            source,
            php,
            captured,
        ),
        "|>" if is_name(right) || right.kind() == "variable_name" => emit(
            right,
            text(right, source),
            if is_name(right) {
                PhpReferenceKind::Function
            } else {
                PhpReferenceKind::Variable
            },
            None,
            true,
            source,
            php,
            captured,
        ),
        _ => {}
    }
}

fn pipe_target(mut node: Node<'_>) -> bool {
    while let Some(parent) = node
        .parent()
        .filter(|parent| parent.kind() == "parenthesized_expression")
    {
        node = parent;
    }
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

fn constant_name(node: Node<'_>) -> bool {
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
        "match_conditional_expression" | "match_default_expression" => parent
            .child_by_field_name("return_expression")
            .is_some_and(|value| value.id() == node.id()),
        "enum_case" | "static_variable_declaration" => parent
            .child_by_field_name("value")
            .is_some_and(|value| value.id() == node.id()),
        "assignment_expression" | "augmented_assignment_expression" => parent
            .child_by_field_name("right")
            .is_some_and(|right| right.id() == node.id()),
        "const_element" => parent
            .named_child(0)
            .is_none_or(|name| name.id() != node.id()),
        "property_element" | "simple_parameter" | "property_promotion_parameter" => parent
            .child_by_field_name("default_value")
            .is_some_and(|value| value.id() == node.id()),
        _ => false,
    }
}

fn emit(
    node: Node<'_>,
    name: &str,
    php_kind: PhpReferenceKind,
    receiver: Option<String>,
    callable: bool,
    source: &[u8],
    php: &mut PhpFacts,
    captured: &mut CapturedTree,
) {
    let namespace = namespace(&php.namespaces, node);
    let owner = ancestors(node)
        .find(|node| is_callable(*node))
        .map(|node| node.start_byte());
    let kind = match php_kind {
        PhpReferenceKind::Class if !callable => ReferenceKind::Type,
        PhpReferenceKind::Method | PhpReferenceKind::StaticMethod if callable => {
            ReferenceKind::Method
        }
        _ if callable => ReferenceKind::Call,
        PhpReferenceKind::Function | PhpReferenceKind::Method | PhpReferenceKind::StaticMethod => {
            ReferenceKind::CallableReference
        }
        _ => ReferenceKind::Value,
    };
    let path = receiver
        .iter()
        .cloned()
        .chain(name_parts(name))
        .collect::<Vec<_>>();
    let scope = ancestors(node)
        .find(|node| is_callable(*node))
        .unwrap_or_else(|| ancestors(node).last().unwrap_or(node));
    captured.references.push(Reference {
        path,
        module: ModulePath(namespace.name.clone()),
        kind,
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        scope_start: scope.start_byte(),
        scope_end: scope.end_byte(),
        call_owner: callable.then_some(owner).flatten(),
        execution_owner: owner,
        access: Some(access(node)),
        context_id: 0,
    });
    php.references.push(PhpReference {
        start_byte: node.start_byte(),
        name: if php_kind == PhpReferenceKind::StaticProperty {
            name.trim_start_matches('$').to_owned()
        } else {
            name.to_owned()
        },
        kind: php_kind,
        receiver: receiver.clone(),
        receiver_reference: receiver
            .as_ref()
            .and_then(|_| node.parent())
            .and_then(|parent| {
                parent
                    .child_by_field_name("object")
                    .or_else(|| parent.child_by_field_name("scope"))
                    .or_else(|| {
                        (parent.kind() == "class_constant_access_expression")
                            .then(|| parent.named_child(0))
                            .flatten()
                    })
            })
            .and_then(receiver_reference),
        namespace_start: namespace.range.start_byte,
        class: class_name(node, source, &php.namespaces),
        callable,
    });
}

fn receiver_reference(node: Node<'_>) -> Option<usize> {
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

fn access(node: Node<'_>) -> ReferenceAccess {
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
