use tree_sitter::{Node, QueryCapture};

use crate::model::{Definition, DefinitionKind, LocalBinding};

use super::scope::{LocalKind, binding_names, is_function, local_scope, parameter_pattern, text};
use super::{CaptureFacts, Captures};

impl CaptureFacts {
    pub(super) fn record_definitions(
        &mut self,
        query: &Captures,
        captures: &[QueryCapture<'_>],
        source: &[u8],
    ) {
        for (capture_id, kind) in &query.definitions {
            for definition in captures.iter().filter(|item| item.index == *capture_id) {
                for name in captures.iter().filter(|item| {
                    Some(item.index) == query.name
                        && super::scope::contains(
                            &definition.node.byte_range(),
                            &item.node.byte_range(),
                        )
                }) {
                    self.add_definition(definition.node, name.node, kind, source);
                }
            }
        }
    }

    fn add_definition(
        &mut self,
        node: Node<'_>,
        name_node: Node<'_>,
        kind: &DefinitionKind,
        source: &[u8],
    ) {
        let callable_value = node.child_by_field_name("value").is_some_and(is_function);
        if *kind == DefinitionKind::Other
            && node.kind() == "variable_declarator"
            && !callable_value
            && !is_module_variable(node)
        {
            return;
        }
        let actual_kind = if node.kind() == "variable_declarator" {
            variable_kind(node, source)
        } else {
            kind.clone()
        };
        let identity_node = node
            .child_by_field_name("value")
            .filter(|value| is_function(*value))
            .unwrap_or(node);
        let range = identity_node.byte_range();
        let private_name = node.kind() == "function_expression"
            && node
                .child_by_field_name("name")
                .is_some_and(|name| name.id() == name_node.id());
        let (scope_start, scope_end) = if private_name {
            (range.start, range.end)
        } else {
            super::scope::scope_range(identity_node, self.root.clone(), false)
        };
        let name_range = name_node.byte_range();
        self.definition_names.push(name_range);
        self.semantic.definitions.push(Definition {
            name: text(name_node, source),
            kind: actual_kind,
            module: crate::model::ModulePath::default(),
            start_byte: range.start,
            end_byte: range.end,
            scope_start,
            scope_end,
            is_public: false,
            inline_module: false,
            external_module: false,
            context_id: 0,
        });
    }

    pub(super) fn record_locals(
        &mut self,
        query: &Captures,
        captures: &[QueryCapture<'_>],
        source: &[u8],
    ) {
        for capture in captures {
            let kind = if Some(capture.index) == query.local_binding {
                Some(LocalKind::Binding)
            } else if Some(capture.index) == query.local_parameter {
                Some(LocalKind::Parameter)
            } else if Some(capture.index) == query.local_catch {
                Some(LocalKind::Catch)
            } else if Some(capture.index) == query.local_loop {
                Some(LocalKind::Loop)
            } else {
                None
            };
            if let Some(kind) = kind {
                self.add_locals(capture.node, kind, source);
            }
        }
    }

    fn add_locals(&mut self, node: Node<'_>, kind: LocalKind, source: &[u8]) {
        let pattern = match kind {
            LocalKind::Parameter => parameter_pattern(node),
            _ => Some(node),
        };
        let Some(pattern) = pattern else { return };
        let (scope_start, scope_end) = local_scope(node, kind, self.root.clone(), source);
        for (name, range) in binding_names(pattern, source) {
            self.semantic.locals.push(LocalBinding {
                name,
                start_byte: range.start,
                end_byte: range.end,
                scope_start,
                scope_end,
                context_id: 0,
            });
        }
    }
}

fn is_module_variable(node: Node<'_>) -> bool {
    let Some(declaration) = node.parent() else {
        return false;
    };
    if !matches!(
        declaration.kind(),
        "lexical_declaration" | "variable_declaration"
    ) {
        return false;
    }
    let Some(parent) = declaration.parent() else {
        return false;
    };
    parent.kind() == "program"
        || (parent.kind() == "export_statement"
            && parent
                .parent()
                .is_some_and(|outer| outer.kind() == "program"))
}

fn variable_kind(node: Node<'_>, source: &[u8]) -> DefinitionKind {
    if node.child_by_field_name("value").is_some_and(is_function) {
        return DefinitionKind::Function;
    }
    let is_const = super::scope::nearest_kind(node, "lexical_declaration")
        .and_then(|declaration| declaration.child_by_field_name("kind"))
        .is_some_and(|kind| String::from_utf8_lossy(&source[kind.byte_range()]) == "const");
    if is_const {
        DefinitionKind::Constant
    } else {
        DefinitionKind::Other
    }
}
