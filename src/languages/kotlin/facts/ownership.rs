use super::*;

pub(super) fn declaration_kind(node: Node<'_>) -> DeclarationKind {
    match node.kind() {
        "function_declaration" | "anonymous_function" | "lambda_literal" => {
            DeclarationKind::Function
        }
        "secondary_constructor" => DeclarationKind::Constructor,
        "class_declaration" => DeclarationKind::Class,
        "object_declaration" => DeclarationKind::Object,
        "companion_object" => DeclarationKind::CompanionObject,
        "property_declaration" => DeclarationKind::Property,
        "type_alias" => DeclarationKind::TypeAlias,
        "getter" | "setter" => DeclarationKind::Accessor,
        _ => DeclarationKind::Other,
    }
}

pub(super) fn definition_kind(node: Node<'_>, owner: &[String]) -> DefinitionKind {
    match node.kind() {
        "class_declaration" => DefinitionKind::Struct,
        "type_alias" => DefinitionKind::TypeAlias,
        "function_declaration" | "anonymous_function" | "lambda_literal" => {
            if owner.is_empty() {
                DefinitionKind::Function
            } else {
                DefinitionKind::Method
            }
        }
        "secondary_constructor" | "getter" | "setter" | "anonymous_initializer" => {
            DefinitionKind::Method
        }
        "property_declaration" => DefinitionKind::Other,
        _ => DefinitionKind::Other,
    }
}

pub(super) fn declaration_name_node(node: Node<'_>) -> Option<Node<'_>> {
    let wanted = match node.kind() {
        "class_declaration" | "object_declaration" | "type_alias" => {
            return node.child_by_field_name("name").or_else(|| {
                named_children(node).into_iter().find(|child| {
                    matches!(
                        child.kind(),
                        "type_identifier" | "simple_identifier" | "identifier"
                    )
                })
            });
        }
        "companion_object" => {
            return node.child_by_field_name("name").or_else(|| {
                named_children(node).into_iter().find(|child| {
                    matches!(
                        child.kind(),
                        "type_identifier" | "simple_identifier" | "identifier"
                    )
                })
            });
        }
        "function_declaration" | "property_declaration" => "simple_identifier",
        "secondary_constructor" => return None,
        _ => return None,
    };
    if node.kind() == "property_declaration" {
        return all_nodes(node)
            .into_iter()
            .filter(|child| child.kind() == "variable_declaration")
            .flat_map(|child| named_children(child).into_iter().collect::<Vec<_>>())
            .find(|child| child.kind() == wanted);
    }
    named_children(node)
        .into_iter()
        .find(|child| child.kind() == wanted)
}

pub(super) fn declaration_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    declaration_name_node(node)
        .map(|name| text(name, source).trim_matches('`').to_owned())
        .or_else(|| (node.kind() == "companion_object").then(|| "Companion".to_owned()))
}

pub(super) fn generated_name(node: Node<'_>) -> String {
    match node.kind() {
        "secondary_constructor" => "constructor".to_owned(),
        "companion_object" => "Companion".to_owned(),
        "getter" => "get".to_owned(),
        "setter" => "set".to_owned(),
        "anonymous_initializer" => "init".to_owned(),
        "anonymous_function" => "<anonymous-function>".to_owned(),
        "lambda_literal" => "<lambda>".to_owned(),
        _ => "<anonymous>".to_owned(),
    }
}

pub(super) fn owner_path(
    node: Node<'_>,
    source: &[u8],
    recovered_types: &[crate::languages::kotlin::syntax::RecoveredType],
) -> Vec<String> {
    let mut owners = node
        .parent()
        .into_iter()
        .flat_map(ancestors)
        .filter(|parent| {
            matches!(
                parent.kind(),
                "class_declaration" | "object_declaration" | "companion_object"
            )
        })
        .filter_map(|parent| {
            declaration_name(parent, source).map(|name| (parent.start_byte(), name))
        })
        .collect::<Vec<_>>();
    owners.extend(
        recovered_types
            .iter()
            .filter(|recovered| {
                recovered.body_range.start <= node.start_byte()
                    && node.end_byte() <= recovered.body_range.end
            })
            .map(|recovered| {
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
                (recovered.declaration_range.start, name)
            }),
    );
    owners.sort_by_key(|(start, _)| *start);
    owners.dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);
    owners.into_iter().map(|(_, name)| name).collect()
}

pub(super) fn nearest_type_declaration(node: Node<'_>) -> Option<Node<'_>> {
    ancestors(node)
        .take_while(|ancestor| !is_executable_scope(ancestor.kind()))
        .find(|parent| {
            matches!(
                parent.kind(),
                "class_declaration" | "object_declaration" | "companion_object"
            )
        })
}

pub(super) fn is_interface_member_declaration(
    node: Node<'_>,
    recovered_types: &[crate::languages::kotlin::syntax::RecoveredType],
) -> bool {
    if nearest_type_declaration(node).is_some_and(|owner| {
        (0..owner.child_count()).any(|index| {
            owner
                .child(index)
                .is_some_and(|child| child.kind() == "interface")
        })
    }) {
        return true;
    }
    let Some(recovered) =
        crate::languages::kotlin::syntax::recovered_type_for(node, recovered_types).filter(
            |recovered| {
                recovered.kind == crate::languages::kotlin::syntax::RecoveredTypeKind::Interface
            },
        )
    else {
        return false;
    };
    ancestors(node)
        .take_while(|ancestor| !is_executable_scope(ancestor.kind()))
        .any(|ancestor| {
            ancestor.kind() == "ERROR" && ancestor.byte_range() == recovered.error_range
        })
}

pub(super) fn declaration_scope(
    node: Node<'_>,
    root: Node<'_>,
    recovered_types: &[crate::languages::kotlin::syntax::RecoveredType],
) -> Range<usize> {
    if let Some(owner) = ancestors(node).find(|ancestor| is_executable_scope(ancestor.kind()))
        && let Some(body) = executable_body(owner)
    {
        return body;
    }
    if let Some(owner) = ancestors(node).find(|ancestor| {
        matches!(
            ancestor.kind(),
            "class_declaration" | "object_declaration" | "companion_object"
        )
    }) {
        return class_body_scope(owner).unwrap_or_else(|| range(owner));
    }
    if let Some(owner) = recovered_types
        .iter()
        .filter(|owner| {
            owner.body_range.start <= node.start_byte() && node.end_byte() <= owner.body_range.end
        })
        .min_by_key(|owner| owner.body_range.end - owner.body_range.start)
    {
        return owner.body_range.clone();
    }
    range(root)
}
