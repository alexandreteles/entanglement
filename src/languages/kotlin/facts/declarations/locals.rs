use super::super::*;

pub(super) fn capture_local_binding(
    node: Node<'_>,
    source: &[u8],
    name: &str,
    name_range: (usize, usize),
    declared_type: Option<&str>,
    result: &mut Capture,
) {
    let Some(outer_callable) =
        ancestors(node).find(|ancestor| is_executable_scope(ancestor.kind()))
    else {
        return;
    };
    let local_scope = ancestors(node)
        .take_while(|ancestor| ancestor.start_byte() >= outer_callable.start_byte())
        .find(|ancestor| ancestor.kind() == "statements")
        .map(range)
        .or_else(|| executable_body(outer_callable))
        .unwrap_or_else(|| range(outer_callable));
    result.locals.push(LocalBinding {
        name: name.to_owned(),
        start_byte: name_range.0,
        end_byte: name_range.1,
        scope_start: local_scope.start,
        scope_end: local_scope.end,
        context_id: 0,
    });

    let (type_name, alias_target, callable_reference, immutable) =
        local_property_metadata(node, source, declared_type);
    result.semantic.bindings.push(BindingType {
        name: name.to_owned(),
        name_start_byte: name_range.0,
        name_end_byte: name_range.1,
        scope_start: local_scope.start,
        scope_end: local_scope.end,
        type_name,
        alias_target,
        callable_reference,
        immutable,
    });
}

pub(super) fn capture_primary_constructor_parameters(
    node: Node<'_>,
    context: &DefinitionContext<'_, '_>,
    sinks: &mut DefinitionSinks<'_>,
    owner: &[String],
    class_name: &str,
) {
    if !matches!(node.kind(), "class_declaration" | "object_declaration") {
        return;
    }
    let Some(parameters) = primary_constructor_parameters(node) else {
        return;
    };
    let source = context.source;
    for parameter in parameters {
        let Some(name_node) = named_children(parameter)
            .into_iter()
            .find(|child| child.kind() == "simple_identifier")
        else {
            continue;
        };
        let type_name = direct_type_children(parameter)
            .into_iter()
            .next()
            .map(|ty| normalized_type(text(ty, source)));
        let is_property = named_children(parameter)
            .into_iter()
            .any(|child| child.kind() == "binding_pattern_kind");
        if is_property {
            emit_constructor_property(
                node,
                parameter,
                context,
                sinks,
                owner
                    .iter()
                    .cloned()
                    .chain(std::iter::once(class_name.to_owned()))
                    .collect(),
                name_node,
                type_name,
            );
        } else if let Some(body) = class_body_scope(node) {
            push_binding(
                sinks.result,
                BindingEvidence {
                    name: text(name_node, source).trim_matches('`').to_owned(),
                    name_node,
                    scope: body,
                    type_name,
                    alias_target: None,
                    callable_reference: false,
                    immutable: true,
                },
            );
            record_declared_name(sinks, name_node);
        }
    }
}

pub(super) fn emit_constructor_property(
    class_node: Node<'_>,
    parameter: Node<'_>,
    context: &DefinitionContext<'_, '_>,
    sinks: &mut DefinitionSinks<'_>,
    owner: Vec<String>,
    name_node: Node<'_>,
    declared_type: Option<String>,
) {
    let source = context.source;
    let class_scope = class_body_scope(class_node).unwrap_or_else(|| range(class_node));
    sinks.result.definitions.push(Definition {
        name: text(name_node, source).trim_matches('`').to_owned(),
        kind: DefinitionKind::Other,
        module: ModulePath(context.package.to_vec()),
        start_byte: parameter.start_byte(),
        end_byte: parameter.end_byte(),
        scope_start: class_scope.start,
        scope_end: class_scope.end,
        is_public: visibility(parameter, source) == SemanticVisibility::Public,
        inline_module: false,
        external_module: false,
        context_id: 0,
    });
    sinks.declarations.push(DeclarationFacts {
        start_byte: parameter.start_byte(),
        owner,
        visibility: visibility(parameter, source),
        kind: DeclarationKind::Property,
        declared_type,
        supertypes: Vec::new(),
        alias_target: None,
        signature: None,
        modifiers: modifiers(parameter, source),
        dsl_markers: Vec::new(),
        is_interface_member: false,
        execution: None,
    });
    record_declared_name(sinks, name_node);
}

pub(super) fn record_declared_name(sinks: &mut DefinitionSinks<'_>, name_node: Node<'_>) {
    sinks
        .declared_names
        .insert((name_node.start_byte(), name_node.end_byte()));
    sinks.excluded.push(name_node.byte_range());
}

pub(super) fn local_property_metadata(
    node: Node<'_>,
    source: &[u8],
    declared_type: Option<&str>,
) -> (Option<String>, Option<String>, bool, bool) {
    if node.kind() != "property_declaration" {
        return (None, None, false, true);
    }
    let Some(variable) = all_nodes(node)
        .into_iter()
        .find(|child| child.kind() == "variable_declaration")
    else {
        return (None, None, false, true);
    };
    let initializer = property_initializer(node, variable);
    let type_name = declared_type
        .map(str::to_owned)
        .or_else(|| initializer.and_then(|value| expression_type(value, source)));
    let callable_reference =
        initializer.is_some_and(|initializer| initializer.kind() == "callable_reference");
    let alias_target = initializer.and_then(|initializer| {
        if callable_reference {
            reference_target(initializer, source).map(|(path, _)| path.join("."))
        } else if matches!(
            initializer.kind(),
            "simple_identifier" | "navigation_expression"
        ) {
            simple_path(initializer, source).map(|path| path.join("."))
        } else {
            None
        }
    });
    let immutable = !named_children(node)
        .into_iter()
        .find(|child| child.kind() == "binding_pattern_kind")
        .is_some_and(|kind| text(kind, source) == "var");
    (type_name, alias_target, callable_reference, immutable)
}
