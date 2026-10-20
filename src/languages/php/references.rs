//! Emit PHP references and keep invocation ownership separate from name lookup.

use tree_sitter::Node;

use crate::graph::PhpReferenceKind::*;
use crate::graph::{PhpReference, PhpReferenceKind};
use crate::model::{ModulePath, Reference, ReferenceAccess, ReferenceKind};

mod expressions;
pub(super) use expressions::receiver_text;
use expressions::{access, constant_name, pipe_target, receiver_reference};

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
        captured.facts.references.push(Reference {
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
