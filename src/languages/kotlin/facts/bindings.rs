use super::*;

pub(super) fn collect_locals(
    source: &[u8],
    nodes: &[Node<'_>],
    declared_names: &HashSet<(usize, usize)>,
    result: &mut Capture,
) {
    for node in nodes.iter().copied() {
        match node.kind() {
            "parameter" | "class_parameter" | "variable_declaration" => {
                collect_value_binding(node, source, declared_names, result);
            }
            _ => {}
        }
    }
}

fn collect_value_binding(
    node: Node<'_>,
    source: &[u8],
    declared_names: &HashSet<(usize, usize)>,
    result: &mut Capture,
) {
    let Some(name_node) = named_children(node)
        .into_iter()
        .find(|child| child.kind() == "simple_identifier")
    else {
        return;
    };
    if declared_names.contains(&(name_node.start_byte(), name_node.end_byte())) {
        return;
    }
    let Some(scope) = binding_scope(node) else {
        return;
    };
    let type_name = direct_type_children(node)
        .next_back()
        .map(|ty| normalized_type(text(ty, source)));
    let (alias_target, callable_reference) = local_alias(node, source);
    push_binding(
        result,
        BindingType {
            name: text(name_node, source).trim_matches('`').to_owned(),
            name_start_byte: name_node.start_byte(),
            name_end_byte: name_node.end_byte(),
            scope_start: scope.start,
            scope_end: scope.end,
            type_name,
            alias_target,
            callable_reference,
            immutable: is_immutable_binding(node, source),
        },
    );
}

fn local_alias(node: Node<'_>, source: &[u8]) -> (Option<String>, bool) {
    if node.kind() != "variable_declaration" {
        return (None, false);
    }
    let Some(property) = node
        .parent()
        .filter(|parent| parent.kind() == "property_declaration")
    else {
        return (None, false);
    };
    let Some(initializer) = property_initializer(property, node) else {
        return (None, false);
    };
    initializer_alias(initializer, source)
}

pub(super) fn initializer_alias(initializer: Node<'_>, source: &[u8]) -> (Option<String>, bool) {
    match initializer.kind() {
        "callable_reference" => (
            reference_target(initializer, source).map(|(path, _)| path.join(".")),
            true,
        ),
        "simple_identifier" | "navigation_expression" => (
            simple_path(initializer, source).map(|path| path.join(".")),
            false,
        ),
        _ => (None, false),
    }
}

fn is_immutable_binding(node: Node<'_>, source: &[u8]) -> bool {
    if node.kind() != "variable_declaration" {
        return true;
    }
    !node
        .parent()
        .filter(|parent| parent.kind() == "property_declaration")
        .and_then(|parent| {
            named_children(parent)
                .into_iter()
                .find(|child| child.kind() == "binding_pattern_kind")
        })
        .is_some_and(|kind| text(kind, source) == "var")
}

pub(super) fn collect_context_facts(
    nodes: &[Node<'_>],
    contexts: &[crate::languages::kotlin::syntax::ContextParameters],
    result: &mut Capture,
) {
    for declaration in nodes.iter().copied().filter(|node| {
        matches!(
            node.kind(),
            "function_declaration"
                | "property_declaration"
                | "class_declaration"
                | "object_declaration"
        )
    }) {
        let Some(context) =
            crate::languages::kotlin::syntax::context_for_declaration(declaration, contexts)
        else {
            continue;
        };
        let Some(scope) = (if matches!(
            declaration.kind(),
            "class_declaration" | "object_declaration"
        ) {
            class_body_scope(declaration)
        } else {
            Some(executable_body(declaration))
        }) else {
            continue;
        };
        for parameter in &context.parameters {
            let type_name = normalized_type(parameter.type_name.clone());
            match (&parameter.name, &parameter.name_range) {
                (None, _) => {
                    let markers = result
                        .definitions
                        .iter()
                        .filter_map(|definition| definition.declaration.as_deref())
                        .find(|fact| {
                            fact.kind == DeclarationKind::Class
                                && fact.start_byte < declaration.start_byte()
                                && fact.declared_type.as_deref().is_some_and(|name| {
                                    name.rsplit('.').next() == type_name.rsplit('.').next()
                                })
                        })
                        .map(|fact| fact.dsl_markers.clone())
                        .unwrap_or_default();
                    result.semantic.receivers.push(ReceiverScope {
                        start_byte: scope.start,
                        end_byte: scope.end,
                        receiver_type: type_name.clone(),
                        implicit: true,
                        label: Some(type_name),
                        dsl_markers: markers,
                    });
                }
                (Some(name), Some(name_range)) => {
                    push_binding(
                        result,
                        BindingType {
                            name: name.clone(),
                            name_start_byte: name_range.start,
                            name_end_byte: name_range.end,
                            scope_start: scope.start,
                            scope_end: scope.end,
                            type_name: Some(type_name),
                            alias_target: None,
                            callable_reference: false,
                            immutable: true,
                        },
                    );
                }
                _ => {}
            }
        }
    }
}

pub(super) fn push_binding(result: &mut Capture, binding: BindingType) {
    result.locals.push(LocalBinding {
        name: binding.name.clone(),
        start_byte: binding.name_start_byte,
        end_byte: binding.name_end_byte,
        scope_start: binding.scope_start,
        scope_end: binding.scope_end,
        context_id: 0,
    });
    result.semantic.bindings.push(binding);
}

pub(super) fn binding_scope(node: Node<'_>) -> Option<Range<usize>> {
    if matches!(node.kind(), "parameter" | "class_parameter") {
        let owner = nearest_declaration(node)?;
        if owner.kind() == "class_declaration" {
            return class_body_scope(owner).or(Some(range(owner)));
        }
        return Some(executable_body(owner));
    }
    if node.kind() == "context_parameter" {
        return nearest_declaration(node).map(executable_body);
    }
    let owner = nearest_declaration(node)?;
    if !is_executable_scope(owner.kind()) {
        return None;
    }
    let scope = ancestors(node)
        .take_while(|ancestor| ancestor.start_byte() >= owner.start_byte())
        .find(|ancestor| {
            matches!(
                ancestor.kind(),
                "statements" | "lambda_literal" | "function_body"
            )
        })
        .map(range)
        .unwrap_or_else(|| executable_body(owner));
    // A local is visible only after its full initializer/declaration.
    let start = if node.kind() == "variable_declaration" {
        node.parent()
            .filter(|parent| parent.kind() == "property_declaration")
            .map_or(node.end_byte(), |parent| parent.end_byte())
    } else {
        node.end_byte()
    };
    Some(start..scope.end)
}

pub(super) fn property_initializer<'tree>(
    property: Node<'tree>,
    declaration: Node<'tree>,
) -> Option<Node<'tree>> {
    named_children(property)
        .into_iter()
        .filter(|child| child.start_byte() >= declaration.end_byte())
        .find(|child| {
            !matches!(
                child.kind(),
                "type_constraints" | "getter" | "setter" | "receiver_type"
            )
        })
}
