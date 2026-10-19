use super::*;

pub(crate) fn declaration_kind(node: Node<'_>) -> DeclarationKind {
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

pub(crate) fn definition_kind(node: Node<'_>, owner: &[String]) -> DefinitionKind {
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
        _ => DefinitionKind::Other,
    }
}

pub(crate) fn declaration_name_node(node: Node<'_>) -> Option<Node<'_>> {
    let wanted = match node.kind() {
        "class_declaration" | "object_declaration" | "type_alias" | "companion_object" => {
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
        _ => return None,
    };
    if node.kind() == "property_declaration" {
        return all_nodes(node)
            .into_iter()
            .filter(|child| child.kind() == "variable_declaration")
            .flat_map(named_children)
            .find(|child| child.kind() == wanted);
    }
    named_children(node)
        .into_iter()
        .find(|child| child.kind() == wanted)
}

pub(crate) fn declaration_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    declaration_name_node(node)
        .map(|name| text(name, source).trim_matches('`').to_owned())
        .or_else(|| (node.kind() == "companion_object").then(|| "Companion".to_owned()))
}

pub(crate) fn generated_name(node: Node<'_>) -> String {
    match node.kind() {
        "secondary_constructor" => "constructor",
        "companion_object" => "Companion",
        "getter" => "get",
        "setter" => "set",
        "anonymous_initializer" => "init",
        "anonymous_function" => "<anonymous-function>",
        "lambda_literal" => "<lambda>",
        _ => "<anonymous>",
    }
    .to_owned()
}

pub(crate) fn owner_path(
    node: Node<'_>,
    source: &[u8],
    recovered_types: &[crate::languages::kotlin::syntax::RecoveredType],
) -> Vec<String> {
    owner_path_for(node, &node.byte_range(), None, source, recovered_types)
}

pub(crate) fn owner_path_for(
    node: Node<'_>,
    declaration_range: &Range<usize>,
    excluded_recovered: Option<&Range<usize>>,
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
                Some(&recovered.declaration_range) != excluded_recovered
                    && recovered.body_range.start <= declaration_range.start
                    && declaration_range.end <= recovered.body_range.end
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

pub(crate) fn nearest_type_declaration(node: Node<'_>) -> Option<Node<'_>> {
    ancestors(node)
        .take_while(|ancestor| !is_executable_scope(ancestor.kind()))
        .find(|parent| {
            matches!(
                parent.kind(),
                "class_declaration" | "object_declaration" | "companion_object"
            )
        })
}

pub(crate) fn is_interface_member_declaration(
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

pub(crate) fn declaration_scope(
    node: Node<'_>,
    root: Node<'_>,
    recovered_types: &[crate::languages::kotlin::syntax::RecoveredType],
) -> Range<usize> {
    if let Some(owner) = ancestors(node).find(|ancestor| is_executable_scope(ancestor.kind())) {
        return executable_body(owner);
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

pub(crate) fn modifier_metadata(
    modifiers: Option<Node<'_>>,
    source: &[u8],
) -> (SemanticVisibility, DeclarationModifiers, Vec<String>) {
    let nodes = modifiers.map(all_nodes).unwrap_or_default();
    let words = nodes
        .iter()
        .copied()
        .filter(|child| child.child_count() == 0)
        .flat_map(|child| [text(child, source), child.kind().to_owned()])
        .collect::<HashSet<_>>();
    let contains = |word: &str| words.contains(word);
    let visibility = if contains("private") {
        SemanticVisibility::Private
    } else if contains("protected") {
        SemanticVisibility::Protected
    } else if contains("internal") {
        SemanticVisibility::Internal
    } else {
        SemanticVisibility::Public
    };
    let flags = DeclarationModifiers {
        is_expect: contains("expect"),
        is_actual: contains("actual"),
        is_abstract: contains("abstract"),
        is_open: contains("open"),
        is_final: contains("final"),
        is_override: contains("override"),
        is_operator: contains("operator"),
        is_infix: contains("infix"),
    };
    let annotations = nodes
        .into_iter()
        .filter(|child| child.kind() == "annotation")
        .flat_map(all_nodes)
        .filter(|child| child.kind() == "user_type")
        .map(|child| normalized_type(text(child, source)))
        .collect();
    (visibility, flags, annotations)
}
