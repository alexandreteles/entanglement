//! Bind declared names as project definitions or block-scope locals.

use std::ops::Range;

use tree_sitter::Node;

use super::super::declarators::{self, QualifiedName};
use super::super::scopes;
use super::{Context, Walker};
use crate::languages::syntax::{named_children, text};
use crate::model::{Definition, DefinitionKind, LocalBinding, ModulePath};

/// What a declarator does with the name it declares.
#[derive(Clone)]
pub(super) enum Binding<'tree> {
    /// Define a project symbol whose source range is the item node.
    Definition {
        kind: DefinitionKind,
        item: Node<'tree>,
        external: bool,
    },
    /// Bind a local name in an explicit scope, or from the name to the end of its block.
    Local(Option<(usize, usize)>),
    /// The name binds nothing, as in a prototype or a function pointer type.
    Ignore,
}

impl<'tree> Walker<'_> {
    /// Bind the declared name and visit the sizes, parameters, and
    /// initializers that the declarator chain contains.
    pub(super) fn declarator(
        &mut self,
        node: Node<'tree>,
        binding: Binding<'tree>,
        context: &Context<'tree>,
    ) {
        match node.kind() {
            "structured_binding_declarator" => {
                for name in named_children(node) {
                    self.bind(name, binding.clone(), context);
                }
            }
            "init_declarator" => {
                self.visit_declarator_field(node, binding, context);
                self.visit_field(node, "value", context);
            }
            "function_declarator"
            | "array_declarator"
            | "pointer_declarator"
            | "reference_declarator"
            | "parenthesized_declarator"
            | "attributed_declarator"
            | "variadic_declarator" => {
                let inner = declarators::inner(node).map(|inner| inner.id());
                for child in named_children(node) {
                    if Some(child.id()) == inner {
                        self.declarator(child, binding.clone(), context);
                    } else {
                        self.visit(child, context);
                    }
                }
            }
            kind if declarators::is_declarator(kind) => self.bind(node, binding, context),
            _ => self.visit(node, context),
        }
    }

    fn visit_declarator_field(
        &mut self,
        node: Node<'tree>,
        binding: Binding<'tree>,
        context: &Context<'tree>,
    ) {
        if let Some(declarator) = node.child_by_field_name("declarator") {
            self.declarator(declarator, binding, context);
        }
    }

    /// Bind each parameter name in `scope`, or ignore the names when no body runs them.
    pub(super) fn parameters(
        &mut self,
        list: Node<'tree>,
        scope: Option<Range<usize>>,
        context: &Context<'tree>,
    ) {
        let binding = match scope {
            Some(scope) => Binding::Local(Some((scope.start, scope.end))),
            None => Binding::Ignore,
        };
        for parameter in named_children(list) {
            self.visit_field(parameter, "type", context);
            self.visit_field(parameter, "default_value", context);
            self.visit_field(parameter, "default_type", context);
            if let Some(declarator) = parameter.child_by_field_name("declarator") {
                self.declarator(declarator, binding.clone(), context);
            } else if let Some(name) = parameter.child_by_field_name("name") {
                self.bind(name, binding.clone(), context);
            } else if parameter.kind().contains("type_parameter") {
                // `typename T` and `typename... T` name their parameter without a field.
                for name in
                    named_children(parameter).filter(|child| child.kind() == "type_identifier")
                {
                    self.bind(name, binding.clone(), context);
                }
            }
        }
    }

    pub(super) fn bind(&mut self, name: Node<'tree>, binding: Binding<'tree>, context: &Context) {
        match binding {
            Binding::Definition {
                kind,
                item,
                external,
            } => self.define(name, kind, item, external, context),
            Binding::Local(scope) => {
                let scope =
                    scope.or_else(|| scopes::from_name(name).map(|range| (range.start, range.end)));
                if let Some((start, end)) = scope {
                    self.local(name, start..end);
                }
            }
            Binding::Ignore => {}
        }
    }

    pub(super) fn define(
        &mut self,
        name: Node<'_>,
        kind: DefinitionKind,
        item: Node<'_>,
        external: bool,
        context: &Context,
    ) {
        let Some(QualifiedName {
            scope,
            name,
            global,
        }) = self.name_of(name)
        else {
            return;
        };
        let mut module = if global {
            Vec::new()
        } else {
            context.module.clone()
        };
        module.extend(scope);
        self.store.definitions.push(Definition {
            declaration: None,
            name,
            kind,
            module: ModulePath(module),
            start_byte: item.start_byte(),
            end_byte: item.end_byte(),
            scope_start: self.root.start,
            scope_end: self.root.end,
            is_public: external && !context.internal,
            inline_module: false,
            external_module: false,
            context_id: 0,
        });
    }

    pub(super) fn local(&mut self, name: Node<'_>, scope: Range<usize>) {
        let spelled = self.name_of(name).map(|name| name.name);
        self.store.locals.push(LocalBinding {
            name: spelled.unwrap_or_else(|| text(name, self.source)),
            start_byte: name.start_byte(),
            end_byte: name.end_byte(),
            scope_start: scope.start,
            scope_end: scope.end,
            context_id: 0,
            ..LocalBinding::default()
        });
    }
}
