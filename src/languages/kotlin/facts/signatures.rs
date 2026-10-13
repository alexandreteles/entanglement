use super::*;

pub(super) fn direct_type_children(node: Node<'_>) -> Vec<Node<'_>> {
    named_children(node)
        .into_iter()
        .filter(|child| is_type_node(child.kind()))
        .collect()
}

pub(super) fn is_type_node(kind: &str) -> bool {
    matches!(
        kind,
        "user_type"
            | "receiver_type"
            | "function_type"
            | "nullable_type"
            | "not_nullable_type"
            | "parenthesized_type"
            | "dynamic"
    )
}

pub(super) fn normalized_type(value: String) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .trim_end_matches('.')
        .to_owned()
}

pub(super) fn declared_type(node: Node<'_>, source: &[u8]) -> Option<String> {
    if node.kind() == "property_declaration" {
        return all_nodes(node)
            .into_iter()
            .find(|child| child.kind() == "variable_declaration")
            .and_then(|variable| direct_type_children(variable).into_iter().next())
            .map(|ty| normalized_type(text(ty, source)));
    }
    if node.kind() == "class_declaration" || node.kind() == "object_declaration" {
        return declaration_name(node, source);
    }
    None
}

pub(super) fn declaration_supertypes(node: Node<'_>, source: &[u8]) -> Vec<String> {
    if !matches!(node.kind(), "class_declaration" | "object_declaration") {
        return Vec::new();
    }
    named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "delegation_specifier")
        .filter_map(|delegation| {
            all_nodes(delegation)
                .into_iter()
                .find(|child| child.kind() == "user_type")
        })
        .map(|child| normalized_type(text(child, source)))
        .collect()
}

pub(super) fn alias_target(node: Node<'_>, source: &[u8]) -> Option<String> {
    direct_type_children(node)
        .into_iter()
        .last()
        .map(|ty| normalized_type(text(ty, source)))
}

pub(super) fn callable_signature(node: Node<'_>, source: &[u8]) -> Option<CallableSignature> {
    if !matches!(
        node.kind(),
        "function_declaration"
            | "secondary_constructor"
            | "class_declaration"
            | "anonymous_function"
            | "property_declaration"
    ) {
        return None;
    }
    let parameter_container = if node.kind() == "class_declaration" {
        primary_constructor_parameters(node)
    } else {
        all_nodes(node)
            .into_iter()
            .find(|child| child.kind() == "function_value_parameters")
            .map(|container| {
                named_children(container)
                    .into_iter()
                    .filter(|child| {
                        matches!(child.kind(), "parameter" | "function_value_parameter")
                    })
                    .collect::<Vec<_>>()
            })
    };
    let parameters = parameter_container
        .unwrap_or_default()
        .into_iter()
        .filter_map(|parameter| parameter_info(parameter, source))
        .collect();
    let receiver_type = node
        .child_by_field_name("receiver")
        .map(|receiver| normalized_type(text(receiver, source)));
    let return_type = if node.kind() == "property_declaration" {
        declared_type(node, source)
    } else if node.kind() == "function_declaration" || node.kind() == "anonymous_function" {
        let params_end = all_nodes(node)
            .into_iter()
            .find(|child| child.kind() == "function_value_parameters")
            .map_or(node.start_byte(), |params| params.end_byte());
        named_children(node)
            .into_iter()
            .find(|child| {
                child.start_byte() >= params_end
                    && is_type_node(child.kind())
                    && child.kind() != "receiver_type"
            })
            .map(|ty| normalized_type(text(ty, source)))
    } else {
        None
    };
    Some(CallableSignature {
        type_parameters: signature_type_parameters(node, source),
        parameters,
        return_type,
        receiver_type,
    })
}

pub(super) fn signature_type_parameters(
    node: Node<'_>,
    source: &[u8],
) -> Vec<CallableTypeParameter> {
    let mut result = Vec::new();
    let class = if node.kind() == "class_declaration" {
        Some(node)
    } else {
        ancestors(node).find(|ancestor| ancestor.kind() == "class_declaration")
    };
    if let Some(class) = class
        && class.id() != node.id()
    {
        append_type_parameters(class, source, &mut result);
    }
    if matches!(
        node.kind(),
        "class_declaration" | "function_declaration" | "anonymous_function"
    ) {
        append_type_parameters(node, source, &mut result);
    }
    result
}

pub(super) fn append_type_parameters(
    declaration: Node<'_>,
    source: &[u8],
    parameters: &mut Vec<CallableTypeParameter>,
) {
    for parameter in named_children(declaration)
        .into_iter()
        .find(|child| child.kind() == "type_parameters")
        .into_iter()
        .flat_map(named_children)
        .filter(|child| child.kind() == "type_parameter")
    {
        let Some(name) = named_children(parameter)
            .into_iter()
            .find(|child| child.kind() == "type_identifier")
            .map(|name| text(name, source).trim_matches('`').to_owned())
        else {
            continue;
        };
        let mut bounds = direct_type_children(parameter)
            .into_iter()
            .map(|bound| normalized_type(text(bound, source)))
            .filter(|bound| !bound.is_empty())
            .collect::<Vec<_>>();
        normalize_bounds(&mut bounds);
        if let Some(existing) = parameters.iter_mut().find(|item| item.name == name) {
            existing.bounds = bounds;
        } else {
            parameters.push(CallableTypeParameter { name, bounds });
        }
    }

    let constraints = named_children(declaration)
        .into_iter()
        .find(|child| child.kind() == "type_constraints");
    for constraint in constraints
        .into_iter()
        .flat_map(named_children)
        .filter(|child| child.kind() == "type_constraint")
    {
        let Some(name) = named_children(constraint)
            .into_iter()
            .find(|child| child.kind() == "type_identifier")
            .map(|name| text(name, source).trim_matches('`').to_owned())
        else {
            continue;
        };
        let Some(parameter) = parameters.iter_mut().find(|item| item.name == name) else {
            continue;
        };
        parameter.bounds.extend(
            direct_type_children(constraint)
                .into_iter()
                .map(|bound| normalized_type(text(bound, source)))
                .filter(|bound| !bound.is_empty()),
        );
        normalize_bounds(&mut parameter.bounds);
    }
}

pub(super) fn normalize_bounds(bounds: &mut Vec<String>) {
    let mut unique = HashSet::new();
    bounds.retain(|bound| unique.insert(bound.clone()));
}

pub(super) fn parameter_info(node: Node<'_>, source: &[u8]) -> Option<CallableParameter> {
    let name = named_children(node)
        .into_iter()
        .find(|child| matches!(child.kind(), "simple_identifier" | "type_identifier"))
        .map(|name| text(name, source).trim_matches('`').to_owned());
    let type_name = direct_type_children(node)
        .into_iter()
        .last()
        .map(|ty| normalized_type(text(ty, source)));
    if name.is_none() && type_name.is_none() {
        return None;
    }
    let node_text = text(node, source);
    let default_gap_has_equals = node.parent().is_some_and(|parent| {
        let next_start = named_children(parent)
            .into_iter()
            .filter(|child| child.start_byte() >= node.end_byte())
            .map(|child| child.start_byte())
            .min()
            .unwrap_or(parent.end_byte());
        source
            .get(node.end_byte()..next_start)
            .is_some_and(|gap| gap.contains(&b'='))
    });
    let parameter_prefix_start = source
        .get(..node.start_byte())
        .and_then(|prefix| prefix.iter().rposition(|byte| matches!(byte, b'(' | b',')))
        .map_or(node.start_byte(), |boundary| boundary + 1);
    let has_vararg_prefix = source
        .get(parameter_prefix_start..node.start_byte())
        .is_some_and(|prefix| String::from_utf8_lossy(prefix).contains("vararg"));
    Some(CallableParameter {
        name,
        type_name,
        has_default: node_text.contains('=') || default_gap_has_equals,
        is_vararg: node_text.contains("vararg")
            || has_vararg_prefix
            || all_nodes(node)
                .into_iter()
                .any(|child| child.kind() == "vararg" || text(child, source) == "vararg"),
    })
}

pub(super) fn primary_constructor_parameters(node: Node<'_>) -> Option<Vec<Node<'_>>> {
    let constructor = named_children(node)
        .into_iter()
        .find(|child| child.kind() == "primary_constructor")?;
    Some(
        all_nodes(constructor)
            .into_iter()
            .filter(|child| child.kind() == "class_parameter")
            .collect(),
    )
}
