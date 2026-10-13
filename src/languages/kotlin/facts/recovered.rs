use super::*;

pub(in crate::languages::kotlin::facts) fn collect_recovered_type_definitions(
    context: &DefinitionContext<'_, '_>,
    result: &mut Capture,
) {
    let root = context.root;
    let nodes = context.nodes;
    let source = context.source;
    let package = context.package;
    let recovered_types = context.recovered_types;
    let scope_index = context.scope_index;
    for recovered in recovered_types {
        let Some(error) = nodes
            .iter()
            .copied()
            .find(|node| node.kind() == "ERROR" && node.byte_range() == recovered.error_range)
        else {
            continue;
        };
        let name = recovered.name_range.as_ref().map_or_else(
            || "Companion".to_owned(),
            |name_range| {
                source
                    .get(name_range.clone())
                    .map(String::from_utf8_lossy)
                    .map(|name| name.trim_matches('`').to_owned())
                    .unwrap_or_default()
            },
        );
        if name.is_empty() {
            continue;
        }
        let start_byte = recovered.declaration_range.start;
        let owner = owner_path_for(
            error,
            &recovered.declaration_range,
            Some(&recovered.declaration_range),
            source,
            recovered_types,
        );
        let kind = match recovered.kind {
            crate::languages::kotlin::syntax::RecoveredTypeKind::Class
            | crate::languages::kotlin::syntax::RecoveredTypeKind::Interface => {
                DeclarationKind::Class
            }
            crate::languages::kotlin::syntax::RecoveredTypeKind::Object => DeclarationKind::Object,
            crate::languages::kotlin::syntax::RecoveredTypeKind::CompanionObject => {
                DeclarationKind::CompanionObject
            }
        };
        let (visibility, mut modifier_flags, markers) =
            modifier_metadata(recovered_modifier_node(error, recovered), source);
        modifier_flags.is_operator = false;
        modifier_flags.is_infix = false;
        let scope = declaration_scope(error, root, recovered_types);
        let definition_kind = match recovered.kind {
            crate::languages::kotlin::syntax::RecoveredTypeKind::Class => DefinitionKind::Struct,
            crate::languages::kotlin::syntax::RecoveredTypeKind::Interface => DefinitionKind::Trait,
            crate::languages::kotlin::syntax::RecoveredTypeKind::Object => DefinitionKind::Struct,
            crate::languages::kotlin::syntax::RecoveredTypeKind::CompanionObject => {
                DefinitionKind::Other
            }
        };
        result.definitions.push(Definition {
            name: name.clone(),
            kind: definition_kind,
            module: ModulePath(package.to_vec()),
            start_byte,
            end_byte: recovered.declaration_range.end,
            scope_start: scope.start,
            scope_end: scope.end,
            is_public: visibility == SemanticVisibility::Public,
            inline_module: false,
            external_module: false,
            context_id: 0,
        });

        if let Some(outer_callable) =
            ancestors(error).find(|ancestor| is_executable_scope(ancestor.kind()))
        {
            let local_scope = executable_body(outer_callable);
            result.locals.push(LocalBinding {
                name: name.clone(),
                start_byte: recovered
                    .name_range
                    .as_ref()
                    .map_or(start_byte, |range| range.start),
                end_byte: recovered
                    .name_range
                    .as_ref()
                    .map_or(start_byte, |range| range.end),
                scope_start: local_scope.start,
                scope_end: local_scope.end,
                context_id: 0,
            });
        }

        let signature = recovered_signature(error, recovered, source);
        result.semantic.declarations.push(DeclarationFacts {
            start_byte,
            owner,
            visibility,
            kind,
            declared_type: Some(name),
            supertypes: recovered_supertypes(error, recovered, source),
            alias_target: None,
            signature,
            modifiers: modifier_flags,
            is_interface_member: false,
            dsl_markers: markers,
            execution: recovered_type_execution(recovered, recovered_types, scope_index),
        });
    }
}

pub(super) fn recovered_header_children<'tree>(
    error: Node<'tree>,
    recovered: &crate::languages::kotlin::syntax::RecoveredType,
) -> Vec<Node<'tree>> {
    named_children(error)
        .into_iter()
        .filter(|child| {
            child.start_byte() >= recovered.keyword_range.end
                && child.end_byte() <= recovered.body_range.start
        })
        .collect()
}

pub(super) fn recovered_supertypes(
    error: Node<'_>,
    recovered: &crate::languages::kotlin::syntax::RecoveredType,
    source: &[u8],
) -> Vec<String> {
    recovered_header_children(error, recovered)
        .into_iter()
        .flat_map(|child| match child.kind() {
            "delegation_specifier" => vec![child],
            "delegation_specifiers" => named_children(child),
            _ => Vec::new(),
        })
        .filter(|child| child.kind() == "delegation_specifier")
        .filter_map(|specifier| {
            all_nodes(specifier)
                .into_iter()
                .find(|node| node.kind() == "user_type")
        })
        .map(|ty| normalized_type(text(ty, source)))
        .collect()
}

pub(super) fn recovered_signature(
    error: Node<'_>,
    recovered: &crate::languages::kotlin::syntax::RecoveredType,
    source: &[u8],
) -> Option<CallableSignature> {
    if !matches!(
        recovered.kind,
        crate::languages::kotlin::syntax::RecoveredTypeKind::Class
            | crate::languages::kotlin::syntax::RecoveredTypeKind::Interface
            | crate::languages::kotlin::syntax::RecoveredTypeKind::Object
    ) {
        return None;
    }
    let header = recovered_header_children(error, recovered);
    let parameters =
        if recovered.kind == crate::languages::kotlin::syntax::RecoveredTypeKind::Interface {
            Vec::new()
        } else {
            header
                .iter()
                .copied()
                .find(|child| child.kind() == "primary_constructor")
                .into_iter()
                .flat_map(all_nodes)
                .filter(|child| child.kind() == "class_parameter")
                .filter_map(|parameter| parameter_info(parameter, source))
                .collect()
        };
    let mut type_parameters = Vec::<CallableTypeParameter>::new();
    for parameter in header
        .iter()
        .copied()
        .filter(|child| child.kind() == "type_parameters")
        .flat_map(named_children)
        .filter(|child| child.kind() == "type_parameter")
    {
        let Some((name, mut bounds)) = type_parameter_parts(parameter, source) else {
            continue;
        };
        normalize_bounds(&mut bounds);
        type_parameters.push(CallableTypeParameter { name, bounds });
    }
    for constraint in header
        .iter()
        .copied()
        .filter(|child| child.kind() == "type_constraints")
        .flat_map(named_children)
        .filter(|child| child.kind() == "type_constraint")
    {
        let Some((name, bounds)) = type_parameter_parts(constraint, source) else {
            continue;
        };
        if let Some(parameter) = type_parameters.iter_mut().find(|item| item.name == name) {
            parameter.bounds.extend(bounds);
            normalize_bounds(&mut parameter.bounds);
        }
    }
    Some(CallableSignature {
        type_parameters,
        parameters,
        return_type: None,
        receiver_type: None,
    })
}

pub(super) fn recovered_modifier_node<'tree>(
    error: Node<'tree>,
    recovered: &crate::languages::kotlin::syntax::RecoveredType,
) -> Option<Node<'tree>> {
    (0..error.child_count())
        .rev()
        .filter_map(|index| error.child(index))
        .filter(|child| child.end_byte() <= recovered.keyword_range.start)
        .find(|child| !matches!(child.kind(), "line_comment" | "multiline_comment"))
        .filter(|child| child.kind() == "modifiers")
}
