use super::*;

mod imports;
mod locals;

pub(super) use imports::*;
use locals::*;

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
    let DefinitionContext {
        root,
        source,
        package,
        contexts,
        recovered_types,
        ..
    } = context;
    let root = *root;
    let source = *source;
    let package = *package;
    let contexts = *contexts;
    let recovered_types = *recovered_types;
    let DefinitionSinks {
        result,
        declarations,
        excluded,
        declared_names,
    } = sinks;
    let result = &mut **result;
    let declarations = &mut **declarations;
    let excluded = &mut **excluded;
    let declared_names = &mut **declared_names;

    let kind = declaration_kind(node);
    let start_byte = crate::languages::kotlin::syntax::context_start(node, contexts);
    let owner = owner_path(node, source, recovered_types);
    let mut name_node = declaration_name_node(node);
    let name = name_node
        .map(|name| text(name, source).trim_matches('`').to_owned())
        .unwrap_or_else(|| generated_name(node));
    let name_range = name_node
        .map(|name| (name.start_byte(), name.end_byte()))
        .unwrap_or((node.start_byte(), node.end_byte()));
    if let Some(name_node) = name_node.take() {
        excluded.push(name_node.byte_range());
        declared_names.insert((name_node.start_byte(), name_node.end_byte()));
    }

    let visibility_level = visibility(node, source);
    let modifier_flags = modifiers(node, source);
    let scope = declaration_scope(node, root, recovered_types);
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
    let markers = annotation_names(node, source);
    declarations.push(DeclarationFacts {
        start_byte,
        owner: owner.clone(),
        visibility: visibility_level,
        kind,
        declared_type: declared_type.clone(),
        supertypes,
        alias_target: alias_target.clone(),
        signature: signature.clone(),
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

    capture_primary_constructor_parameters(
        node,
        context,
        &mut DefinitionSinks {
            result: &mut *result,
            declarations: &mut *declarations,
            excluded: &mut *excluded,
            declared_names: &mut *declared_names,
        },
        &owner,
        &name,
    );

    if let Some(receiver) = node.child_by_field_name("receiver")
        && let Some(body) = executable_body(node)
    {
        result.semantic.receivers.push(ReceiverScope {
            start_byte: body.start,
            end_byte: body.end,
            receiver_type: normalized_type(text(receiver, source)),
            implicit: true,
            label: declaration_name(node, source),
            dsl_markers: Vec::new(),
        });
    }

    if (node.kind() == "class_declaration"
        || node.kind() == "object_declaration"
        || node.kind() == "companion_object")
        && let Some(body) = class_body_scope(node)
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
}
