//! Visit lambdas, range loops, and catch clauses, which bind names for one block.

use tree_sitter::Node;

use super::bindings::Binding;
use super::{Context, Walker};
use crate::languages::syntax::named_children;

impl<'tree> Walker<'_> {
    pub(super) fn lambda(&mut self, node: Node<'tree>, context: &Context<'tree>) {
        let inner = context.in_function(node);
        self.visit_field(node, "captures", context);
        for field in ["template_parameters", "declarator"] {
            let Some(child) = node.child_by_field_name(field) else {
                continue;
            };
            let list = if field == "declarator" {
                child.child_by_field_name("parameters")
            } else {
                Some(child)
            };
            if let Some(list) = list {
                self.parameters(list, Some(node.byte_range()), &inner);
            }
            for rest in named_children(child).filter(|rest| Some(*rest) != list) {
                self.visit(rest, &inner);
            }
        }
        self.visit_field(node, "body", &inner);
    }

    pub(super) fn range_loop(&mut self, node: Node<'tree>, context: &Context<'tree>) {
        self.visit_field(node, "initializer", context);
        self.visit_field(node, "type", context);
        if let Some(declarator) = node.child_by_field_name("declarator") {
            self.declarator(declarator, Binding::Local(None), context);
        }
        self.visit_field(node, "right", context);
        self.visit_field(node, "body", context);
    }

    pub(super) fn catch_clause(&mut self, node: Node<'tree>, context: &Context<'tree>) {
        if let Some(parameters) = node.child_by_field_name("parameters") {
            self.parameters(parameters, Some(node.byte_range()), context);
        }
        self.visit_field(node, "body", context);
    }
}
