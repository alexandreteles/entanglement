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
            "context_parameter" => {
                collect_context_parameter(node, source, result);
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
        BindingEvidence {
            name: text(name_node, source).trim_matches('`').to_owned(),
            name_node,
            scope,
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

fn collect_context_parameter(node: Node<'_>, source: &[u8], result: &mut Capture) {
    let Some(scope) = binding_scope(node) else {
        return;
    };
    let name_node = named_children(node)
        .into_iter()
        .find(|child| child.kind() == "simple_identifier");
    let type_node = direct_type_children(node).next_back();
    if let Some(name_node) = name_node {
        push_binding(
            result,
            BindingEvidence {
                name: text(name_node, source).trim_matches('`').to_owned(),
                name_node,
                scope,
                type_name: type_node.map(|ty| normalized_type(text(ty, source))),
                alias_target: None,
                callable_reference: false,
                immutable: true,
            },
        );
    } else {
        collect_implicit_context_receiver(node, type_node, source, result);
    }
}

fn collect_implicit_context_receiver(
    node: Node<'_>,
    type_node: Option<Node<'_>>,
    source: &[u8],
    result: &mut Capture,
) {
    let Some((ty, owner)) = type_node.zip(nearest_declaration(node)) else {
        return;
    };
    let ty = normalized_type(text(ty, source));
    let body = executable_body(owner);
    result.semantic.receivers.push(ReceiverScope {
        start_byte: body.start,
        end_byte: body.end,
        receiver_type: ty.clone(),
        implicit: true,
        label: Some(ty),
        dsl_markers: Vec::new(),
    });
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
            if parameter.name.is_none() {
                let markers = result
                    .semantic
                    .declarations
                    .iter()
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
                    label: Some(type_name.clone()),
                    dsl_markers: markers,
                });
            }
            if let (Some(name), Some(name_range)) = (&parameter.name, &parameter.name_range) {
                result.locals.push(LocalBinding {
                    name: name.clone(),
                    start_byte: name_range.start,
                    end_byte: name_range.end,
                    scope_start: scope.start,
                    scope_end: scope.end,
                    context_id: 0,
                });
                result.semantic.bindings.push(BindingType {
                    name: name.clone(),
                    name_start_byte: name_range.start,
                    name_end_byte: name_range.end,
                    scope_start: scope.start,
                    scope_end: scope.end,
                    type_name: Some(type_name),
                    alias_target: None,
                    callable_reference: false,
                    immutable: true,
                });
            }
        }
    }
}

pub(super) fn push_binding(result: &mut Capture, evidence: BindingEvidence<'_>) {
    result.locals.push(LocalBinding {
        name: evidence.name.clone(),
        start_byte: evidence.name_node.start_byte(),
        end_byte: evidence.name_node.end_byte(),
        scope_start: evidence.scope.start,
        scope_end: evidence.scope.end,
        context_id: 0,
    });
    result.semantic.bindings.push(BindingType {
        name: evidence.name,
        name_start_byte: evidence.name_node.start_byte(),
        name_end_byte: evidence.name_node.end_byte(),
        scope_start: evidence.scope.start,
        scope_end: evidence.scope.end,
        type_name: evidence.type_name,
        alias_target: evidence.alias_target,
        callable_reference: evidence.callable_reference,
        immutable: evidence.immutable,
    });
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
