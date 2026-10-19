use super::*;

mod constructors;
mod identity;
mod imports;

pub(super) use constructors::*;
pub(super) use identity::*;
pub(super) use imports::*;

pub(super) fn is_definition_node(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "class_declaration"
            | "object_declaration"
            | "companion_object"
            | "function_declaration"
            | "secondary_constructor"
            | "property_declaration"
            | "type_alias"
            | "anonymous_function"
            | "lambda_literal"
            | "getter"
            | "setter"
            | "anonymous_initializer"
    )
}

pub(super) fn collect_definition(
    node: Node<'_>,
    context: &DefinitionContext<'_, '_>,
    sinks: &mut DefinitionSinks<'_>,
) {
    let source = context.source;
    let package = context.package;
    let contexts = context.contexts;
    let recovered_types = context.recovered_types;
    let kind = declaration_kind(node);
    let start_byte = crate::languages::kotlin::syntax::context_start(node, contexts);
    let owner = owner_path(node, source, recovered_types);
    let name_node = declaration_name_node(node);
    let name = name_node
        .map(|name| text(name, source).trim_matches('`').to_owned())
        .unwrap_or_else(|| generated_name(node));
    let name_range = name_node
        .map(|name| (name.start_byte(), name.end_byte()))
        .unwrap_or((node.start_byte(), node.end_byte()));
    if let Some(name_node) = name_node {
        record_declared_name(sinks, name_node);
    }
    let result = &mut *sinks.result;

    let modifier_node = named_children(node)
        .into_iter()
        .find(|child| child.kind() == "modifiers");
    let (visibility_level, modifier_flags, markers) = modifier_metadata(modifier_node, source);
    let scope = declaration_scope(node, context.root, recovered_types);
    let definition_kind = definition_kind(node, &owner);
    let is_interface_member = is_interface_member_declaration(node, recovered_types);
    let module = ModulePath(package.to_vec());

    result.definitions.push(Definition {
        name: name.clone(),
        kind: definition_kind,
        module,
        start_byte,
        end_byte: node.end_byte(),
        scope_start: scope.start,
        scope_end: scope.end,
        is_public: visibility_level == SemanticVisibility::Public,
        inline_module: false,
        external_module: false,
        context_id: 0,
    });

    let signature = callable_signature(node, source);
    let declared_type = declared_type(node, source);
    let supertypes = declaration_supertypes(node, source);
    let alias_target = (node.kind() == "type_alias")
        .then(|| alias_target(node, source))
        .flatten();
    // Keep raw annotations so the resolver can establish DslMarker metadata
    // across files, even when the annotation type is declared elsewhere.
    result.semantic.declarations.push(DeclarationFacts {
        php: None,
        start_byte,
        owner: owner.clone(),
        visibility: visibility_level,
        kind,
        declared_type: declared_type.clone(),
        supertypes,
        alias_target,
        signature,
        modifiers: modifier_flags,
        dsl_markers: markers.clone(),
        is_interface_member,
        execution: declaration_execution(node, context.scope_index),
    });

    capture_local_binding(
        node,
        source,
        &name,
        name_range,
        declared_type.as_deref(),
        result,
    );

    if let Some(receiver) = node.child_by_field_name("receiver") {
        let body = executable_body(node);
        result.semantic.receivers.push(ReceiverScope {
            start_byte: body.start,
            end_byte: body.end,
            receiver_type: normalized_type(text(receiver, source)),
            implicit: true,
            label: declaration_name(node, source),
            dsl_markers: Vec::new(),
        });
    }

    if matches!(
        node.kind(),
        "class_declaration" | "object_declaration" | "companion_object"
    ) && let Some(body) = class_body_scope(node)
    {
        let receiver_type = package
            .iter()
            .chain(&owner)
            .chain(std::iter::once(&name))
            .cloned()
            .collect::<Vec<_>>()
            .join(".");
        result.semantic.receivers.push(ReceiverScope {
            start_byte: body.start,
            end_byte: body.end,
            receiver_type,
            implicit: true,
            label: Some(name.clone()),
            dsl_markers: markers,
        });
    }
    capture_primary_constructor_parameters(node, context, sinks, &owner, &name);
}

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
        .unwrap_or_else(|| executable_body(outer_callable));
    let (type_name, alias_target, callable_reference, immutable) =
        local_property_metadata(node, source, declared_type);
    push_binding(
        result,
        BindingType {
            name: name.to_owned(),
            name_start_byte: name_range.0,
            name_end_byte: name_range.1,
            scope_start: local_scope.start,
            scope_end: local_scope.end,
            type_name,
            alias_target,
            callable_reference,
            immutable,
        },
    );
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
    let (alias_target, callable_reference) = initializer
        .map(|initializer| initializer_alias(initializer, source))
        .unwrap_or_default();
    let immutable = !named_children(node)
        .into_iter()
        .find(|child| child.kind() == "binding_pattern_kind")
        .is_some_and(|kind| text(kind, source) == "var");
    (type_name, alias_target, callable_reference, immutable)
}
