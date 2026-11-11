//! Visit struct, union, class, and enum specifiers and type aliases.

use tree_sitter::Node;

use super::super::declarators::qualified_name;
use super::bindings::Binding;
use super::{Context, Walker};
use crate::languages::syntax::named_children;
use crate::model::DefinitionKind;

impl<'tree> Walker<'_> {
    /// A specifier without a body, such as `struct node *next`, refers to its tag.
    pub(super) fn type_specifier(&mut self, node: Node<'tree>, context: &Context<'tree>) {
        let name = node.child_by_field_name("name");
        let Some(body) = node.child_by_field_name("body") else {
            if let Some(name) = name {
                self.visit(name, context);
            }
            return;
        };
        if let Some(name) = name {
            self.bind_type(name, type_kind(node.kind()), node, context);
        }
        for clause in named_children(node).filter(|child| child.kind() == "base_class_clause") {
            self.visit(clause, context);
        }
        let mut inner = context.clone();
        let is_enum = node.kind() == "enum_specifier";
        if self.cpp && (!is_enum || is_scoped_enum(node)) {
            if let Some(name) = name.and_then(|name| qualified_name(name, self.source)) {
                inner.module.extend(name.scope);
                inner.module.push(name.name);
            }
            inner.class = !is_enum;
        }
        if is_enum {
            self.enumerators(body, &inner);
        } else {
            self.visit(body, &inner);
        }
    }

    fn enumerators(&mut self, body: Node<'tree>, context: &Context<'tree>) {
        for enumerator in named_children(body).filter(|child| child.kind() == "enumerator") {
            if let Some(name) = enumerator.child_by_field_name("name") {
                self.bind_type(name, DefinitionKind::Constant, enumerator, context);
            }
            self.visit_field(enumerator, "value", context);
        }
    }

    pub(super) fn type_definition(&mut self, node: Node<'tree>, context: &Context<'tree>) {
        self.visit_field(node, "type", context);
        let mut cursor = node.walk();
        let declared = node
            .children_by_field_name("declarator", &mut cursor)
            .collect::<Vec<_>>();
        for declarator in declared {
            let binding = if context.is_block_scope() {
                Binding::Local(None)
            } else {
                Binding::Definition {
                    kind: DefinitionKind::TypeAlias,
                    item: node,
                    external: true,
                }
            };
            self.declarator(declarator, binding, context);
        }
    }

    pub(super) fn alias_declaration(&mut self, node: Node<'tree>, context: &Context<'tree>) {
        if let Some(name) = node.child_by_field_name("name") {
            self.bind_type(name, DefinitionKind::TypeAlias, node, context);
        }
        self.visit_field(node, "type", context);
    }

    fn bind_type(
        &mut self,
        name: Node<'tree>,
        kind: DefinitionKind,
        item: Node<'tree>,
        context: &Context<'tree>,
    ) {
        if context.is_block_scope() {
            self.bind(name, Binding::Local(None), context);
        } else {
            self.define(name, kind, item, true, context);
        }
    }
}

fn type_kind(kind: &str) -> DefinitionKind {
    match kind {
        "union_specifier" => DefinitionKind::Union,
        "enum_specifier" => DefinitionKind::Enum,
        _ => DefinitionKind::Struct,
    }
}

/// `enum class Color` keeps its enumerators inside the enum's scope.
fn is_scoped_enum(node: Node<'_>) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| matches!(child.kind(), "class" | "struct"))
}
