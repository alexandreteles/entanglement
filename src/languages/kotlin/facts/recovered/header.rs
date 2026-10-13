use super::super::*;

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
    let mut result = Vec::new();
    for child in recovered_header_children(error, recovered) {
        if child.kind() == "delegation_specifier" {
            if let Some(user_type) = all_nodes(child)
                .into_iter()
                .find(|node| node.kind() == "user_type")
            {
                result.push(normalized_type(text(user_type, source)));
            }
        } else if child.kind() == "delegation_specifiers" {
            result.extend(
                named_children(child)
                    .into_iter()
                    .filter(|specifier| specifier.kind() == "delegation_specifier")
                    .filter_map(|specifier| {
                        all_nodes(specifier)
                            .into_iter()
                            .find(|node| node.kind() == "user_type")
                    })
                    .map(|user_type| normalized_type(text(user_type, source))),
            );
        }
    }
    result
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
            .collect::<Vec<_>>();
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
        let Some(name) = named_children(constraint)
            .into_iter()
            .find(|child| child.kind() == "type_identifier")
            .map(|name| text(name, source).trim_matches('`').to_owned())
        else {
            continue;
        };
        if let Some(parameter) = type_parameters.iter_mut().find(|item| item.name == name) {
            parameter.bounds.extend(
                direct_type_children(constraint)
                    .into_iter()
                    .map(|bound| normalized_type(text(bound, source))),
            );
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
