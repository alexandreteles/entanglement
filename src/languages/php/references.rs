//! Emit PHP references and keep invocation ownership separate from name lookup.

use tree_sitter::Node;

use crate::graph::PhpReferenceKind::*;
use crate::graph::{PhpReference, PhpReferenceKind};
use crate::model::{ModulePath, Reference, ReferenceAccess, ReferenceKind};

use super::Capture;
use super::syntax::*;

impl Capture<'_> {
    pub(super) fn reference(&mut self, node: Node<'_>) {
        match node.kind() {
            "function_call_expression" => self.function(node),
            "object_creation_expression" => self.construction(node),
            "member_call_expression"
            | "nullsafe_member_call_expression"
            | "scoped_call_expression"
            | "member_access_expression"
            | "nullsafe_member_access_expression"
            | "scoped_property_access_expression" => self.member(node),
            "class_constant_access_expression" => self.class_constant(node),
            "named_type" | "base_clause" | "class_interface_clause" => {
                for name in children(node).filter(|name| is_name(*name)) {
                    self.emit_reference(name, text(name, self.source), Class, None, false);
                }
            }
            "attribute" => {
                if let Some(name) = children(node).find(|name| is_name(*name)) {
                    self.emit_reference(name, text(name, self.source), Class, None, false);
                }
            }
            "binary_expression" => self.binary(node),
            "name" | "qualified_name" | "relative_name"
                if constant_name(node)
                    && !self
                        .php
                        .references
                        .iter()
                        .any(|reference| reference.start_byte == node.start_byte()) =>
            {
                self.emit_reference(node, text(node, self.source), Constant, None, false);
            }
            _ => {}
        }
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
        if let Some(kind) = children(node).find(|child| is_name(*child)) {
            return format!("new {}", text(kind, source));
        }
    }
    text(node, source).to_owned()
}

fn pipe_target(node: Node<'_>) -> bool {
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

impl Capture<'_> {
    fn function(&mut self, node: Node<'_>) {
        let source = self.source;
        let Some(target) = node.child_by_field_name("function").map(unwrap) else {
            return;
        };
        let (target, name, kind) = match target.kind() {
            "anonymous_function" | "arrow_function" => {
                let Some(arguments) = node.child_by_field_name("arguments") else {
                    return;
                };
                (arguments, declaration_name(target, source), Function)
            }
            _ => match literal(target, source) {
                Some(name) => (
                    target,
                    format!("\\{}", name.trim_start_matches('\\')),
                    Function,
                ),
                None => (
                    target,
                    text(target, source).to_owned(),
                    if is_name(target) { Function } else { Variable },
                ),
            },
        };
        self.emit_reference(
            target,
            &name,
            kind,
            None,
            !first_class_callable(node) || pipe_target(node),
        );
    }

    fn construction(&mut self, node: Node<'_>) {
        let source = self.source;
        let Some(target) = children(node).find(|child| {
            is_name(*child) || matches!(child.kind(), "variable_name" | "anonymous_class")
        }) else {
            return;
        };
        let name = if target.kind() == "anonymous_class" {
            format!(
                "\\{}",
                namespace(&self.php.namespaces, target).qualify(&declaration_name(target, source))
            )
        } else {
            text(target, source).to_owned()
        };
        self.emit_reference(target, &name, Class, None, true);
    }

    fn member(&mut self, node: Node<'_>) {
        let source = self.source;
        let Some(name) = node.child_by_field_name("name") else {
            return;
        };
        let scoped = node.kind().starts_with("scoped_");
        let Some(receiver) = node.child_by_field_name(if scoped { "scope" } else { "object" })
        else {
            return;
        };
        let call = node.kind().contains("call_expression");
        let kind = match (call, scoped) {
            (true, true) => StaticMethod,
            (true, false) => Method,
            (false, true) => StaticProperty,
            (false, false) => Property,
        };
        self.emit_reference(
            name,
            text(name, source),
            kind,
            Some(receiver),
            call && (!first_class_callable(node) || pipe_target(node)),
        );
        if scoped && is_name(receiver) {
            self.emit_reference(receiver, text(receiver, source), Class, None, false);
        }
    }

    fn class_constant(&mut self, node: Node<'_>) {
        let source = self.source;
        if node.parent().is_some_and(|parent| {
            matches!(parent.kind(), "use_as_clause" | "use_instead_of_clause")
        }) {
            return;
        }
        let mut children = children(node);
        let (Some(receiver), Some(name)) = (children.next(), children.next_back()) else {
            return;
        };
        if !text(name, source).eq_ignore_ascii_case("class") {
            self.emit_reference(
                name,
                text(name, source),
                ClassConstant,
                Some(receiver),
                false,
            );
        }
        if is_name(receiver) {
            self.emit_reference(receiver, text(receiver, source), Class, None, false);
        }
    }

    fn binary(&mut self, node: Node<'_>) {
        let source = self.source;
        let Some(operator) = node.child_by_field_name("operator") else {
            return;
        };
        let Some(right) = node.child_by_field_name("right").map(unwrap) else {
            return;
        };
        match text(operator, source) {
            "|>" if matches!(right.kind(), "anonymous_function" | "arrow_function") => self
                .emit_reference(
                    operator,
                    &declaration_name(right, source),
                    Function,
                    None,
                    true,
                ),
            "instanceof" if is_name(right) => {
                self.emit_reference(right, text(right, source), Class, None, false)
            }
            "|>" if is_name(right) || right.kind() == "variable_name" => self.emit_reference(
                right,
                text(right, source),
                if is_name(right) { Function } else { Variable },
                None,
                true,
            ),
            _ => {}
        }
    }

    fn emit_reference(
        &mut self,
        node: Node<'_>,
        name: &str,
        php_kind: PhpReferenceKind,
        receiver: Option<Node<'_>>,
        callable: bool,
    ) {
        let source = self.source;
        let php = &mut *self.php;
        let captured = &mut *self.captured;
        let scope = scope(node);
        let owner = is_callable(scope).then(|| scope.start_byte());
        let kind = match php_kind {
            Class if !callable => ReferenceKind::Type,
            Method | StaticMethod if callable => ReferenceKind::Method,
            _ if callable => ReferenceKind::Call,
            Function | Method | StaticMethod => ReferenceKind::CallableReference,
            _ => ReferenceKind::Value,
        };
        let receiver_name = receiver.map(|node| receiver_text(node, source));
        let path = receiver_name
            .iter()
            .cloned()
            .chain(name_parts(name))
            .collect::<Vec<_>>();
        captured.references.push(Reference {
            path,
            module: ModulePath::default(),
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
            name: if php_kind == StaticProperty {
                name.trim_start_matches('$').to_owned()
            } else {
                name.to_owned()
            },
            kind: php_kind,
            receiver: receiver_name,
            receiver_reference: receiver.and_then(receiver_reference),
            class: class_name(node, source, &php.namespaces),
            callable,
        });
    }
}
