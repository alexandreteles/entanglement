//! Visit functions, variables, namespaces, templates, and preprocessor items.

use tree_sitter::Node;

use super::super::declarators::{self, qualified_name};
use super::bindings::Binding;
use super::{Context, Walker};
use crate::languages::syntax::{named_children, text};
use crate::model::DefinitionKind;

impl<'tree> Walker<'_> {
    pub(super) fn function_definition(&mut self, node: Node<'tree>, context: &Context<'tree>) {
        self.visit_field(node, "type", context);
        let declarator = node.child_by_field_name("declarator");
        let mut inner = context.in_function(node);
        if let Some(name) = declarator.and_then(declarators::name) {
            let qualified = qualified_name(name, self.source);
            let scope = qualified.map(|name| name.scope).unwrap_or_default();
            // An out-of-line definition such as `Shape::area` cannot be told apart
            // from a namespace function, so both stay methods without proven dispatch.
            let kind = if context.class || !scope.is_empty() {
                DefinitionKind::Method
            } else {
                DefinitionKind::Function
            };
            let external = context.class || !has_storage(node, "static");
            self.bind_function(name, kind, node, external, context);
            inner.module.extend(scope);
        }
        if let Some(function) = declarator.and_then(declarators::function_declarator) {
            let parameters = function.child_by_field_name("parameters");
            if let Some(parameters) = parameters {
                self.parameters(
                    parameters,
                    Some(parameters.end_byte()..node.end_byte()),
                    &inner,
                );
            }
            let skipped = [function.child_by_field_name("declarator"), parameters];
            for child in named_children(function).filter(|child| !skipped.contains(&Some(*child))) {
                self.visit(child, &inner);
            }
        }
        for child in named_children(node).filter(|child| {
            matches!(
                child.kind(),
                "field_initializer_list" | "compound_statement" | "try_statement"
            )
        }) {
            self.visit(child, &inner);
        }
    }

    fn bind_function(
        &mut self,
        name: Node<'tree>,
        kind: DefinitionKind,
        item: Node<'tree>,
        external: bool,
        context: &Context<'tree>,
    ) {
        if context.is_block_scope() {
            self.bind(name, Binding::Local(None), context);
        } else {
            self.define(name, kind, item, external, context);
        }
    }

    pub(super) fn declaration(&mut self, node: Node<'tree>, context: &Context<'tree>) {
        self.visit_field(node, "type", context);
        let mut cursor = node.walk();
        let declared = node
            .children_by_field_name("declarator", &mut cursor)
            .collect::<Vec<_>>();
        for declarator in declared {
            let binding = self.declaration_binding(node, declarator, context);
            self.declarator(declarator, binding, context);
        }
        self.visit_field(node, "default_value", context);
        for child in named_children(node).filter(|child| child.kind() == "bitfield_clause") {
            self.visit(child, context);
        }
    }

    /// Prototypes and `extern` declarations name symbols that are defined elsewhere.
    fn declaration_binding(
        &self,
        node: Node<'tree>,
        declarator: Node<'tree>,
        context: &Context<'tree>,
    ) -> Binding<'tree> {
        let is_field = node.kind() == "field_declaration";
        let is_extern = has_storage(node, "extern") && declarator.kind() != "init_declarator";
        if declarators::declares_function(declarator) || is_extern || (is_field && !self.cpp) {
            return Binding::Ignore;
        }
        if context.is_block_scope() {
            return Binding::Local(None);
        }
        Binding::Definition {
            kind: if is_field {
                DefinitionKind::Other
            } else {
                DefinitionKind::Static
            },
            item: node,
            external: context.class || !has_storage(node, "static"),
        }
    }

    pub(super) fn namespace_definition(&mut self, node: Node<'tree>, context: &Context<'tree>) {
        let mut inner = context.clone();
        match node.child_by_field_name("name") {
            Some(name) => namespace_names(name, self.source, &mut inner.module),
            None => inner.internal = true,
        }
        self.visit_field(node, "body", &inner);
    }

    pub(super) fn template_declaration(&mut self, node: Node<'tree>, context: &Context<'tree>) {
        let parameters = node.child_by_field_name("parameters");
        if let Some(parameters) = parameters {
            self.parameters(parameters, Some(node.byte_range()), context);
        }
        for child in named_children(node).filter(|child| Some(*child) != parameters) {
            self.visit(child, context);
        }
    }
}

pub(super) fn has_storage(node: Node<'_>, keyword: &str) -> bool {
    named_children(node).any(|child| {
        child.kind() == "storage_class_specifier"
            && child.child(0).is_some_and(|word| word.kind() == keyword)
    })
}

/// `namespace a::b` opens both namespaces.
fn namespace_names(node: Node<'_>, source: &[u8], module: &mut Vec<String>) {
    if node.kind() == "namespace_identifier" {
        module.push(text(node, source));
    }
    for child in named_children(node) {
        namespace_names(child, source, module);
    }
}
