use super::*;

pub(crate) fn capture_primary_constructor_parameters(
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
    let class_body = class_body_scope(node);
    for parameter in parameters {
        let Some(name_node) = named_children(parameter)
            .into_iter()
            .find(|child| child.kind() == "simple_identifier")
        else {
            continue;
        };
        let type_name = direct_type_children(parameter)
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
        } else if let Some(body) = class_body.clone() {
            push_binding(
                sinks.result,
                BindingType {
                    name: text(name_node, source).trim_matches('`').to_owned(),
                    name_start_byte: name_node.start_byte(),
                    name_end_byte: name_node.end_byte(),
                    scope_start: body.start,
                    scope_end: body.end,
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

pub(crate) fn emit_constructor_property(
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
    let modifier_node = named_children(parameter)
        .into_iter()
        .find(|child| child.kind() == "modifiers");
    let (visibility, modifiers, _) = modifier_metadata(modifier_node, source);

    let declaration = DeclarationFacts {
        start_byte: parameter.start_byte(),
        owner,
        visibility,
        kind: DeclarationKind::Property,
        declared_type,
        modifiers,
        ..DeclarationFacts::default()
    };
    sinks.result.definitions.push(Definition {
        declaration: Some(Box::new(declaration)),
        name: text(name_node, source).trim_matches('`').to_owned(),
        kind: DefinitionKind::Other,
        module: ModulePath(context.package.to_vec()),
        start_byte: parameter.start_byte(),
        end_byte: parameter.end_byte(),
        scope_start: class_scope.start,
        scope_end: class_scope.end,
        is_public: visibility == SemanticVisibility::Public,
        inline_module: false,
        external_module: false,
        context_id: 0,
    });
    record_declared_name(sinks, name_node);
}
