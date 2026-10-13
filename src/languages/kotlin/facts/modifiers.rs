use super::*;

pub(super) fn visibility(node: Node<'_>, source: &[u8]) -> SemanticVisibility {
    if has_modifier(node, source, "private") {
        SemanticVisibility::Private
    } else if has_modifier(node, source, "protected") {
        SemanticVisibility::Protected
    } else if has_modifier(node, source, "internal") {
        SemanticVisibility::Internal
    } else {
        SemanticVisibility::Public
    }
}

pub(super) fn modifiers(node: Node<'_>, source: &[u8]) -> DeclarationModifiers {
    let contains = |value: &str| has_modifier(node, source, value);
    DeclarationModifiers {
        is_expect: contains("expect"),
        is_actual: contains("actual"),
        is_abstract: contains("abstract"),
        is_open: contains("open"),
        is_final: contains("final"),
        is_override: contains("override"),
        is_operator: contains("operator"),
        is_infix: contains("infix"),
    }
}

pub(super) fn has_modifier(node: Node<'_>, source: &[u8], value: &str) -> bool {
    named_children(node)
        .into_iter()
        .find(|child| child.kind() == "modifiers")
        .is_some_and(|modifiers| {
            all_nodes(modifiers)
                .into_iter()
                .filter(|child| child.child_count() == 0)
                .any(|child| text(child, source) == value || child.kind() == value)
        })
}

pub(super) fn annotation_names(node: Node<'_>, source: &[u8]) -> Vec<String> {
    named_children(node)
        .into_iter()
        .find(|child| child.kind() == "modifiers")
        .into_iter()
        .flat_map(all_nodes)
        .filter(|child| child.kind() == "annotation")
        .flat_map(|annotation| all_nodes(annotation))
        .filter(|child| child.kind() == "user_type")
        .map(|child| normalized_type(text(child, source)))
        .collect()
}

pub(super) fn has_annotation(node: Node<'_>, source: &[u8], name: &str) -> bool {
    annotation_names(node, source)
        .iter()
        .any(|annotation| annotation.rsplit('.').next() == Some(name))
}
