use tree_sitter::Node;

use crate::model::ReferenceKind;

use super::super::syntax::{ancestors, children, field, range, text};

pub(super) fn path_segments(node: Node<'_>, source: &[u8]) -> Vec<String> {
    match node.kind() {
        "identifier" => vec![text(node, source)],
        "dotted_name" => text(node, source)
            .split('.')
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect(),
        "attribute" => attribute_segments(node, source),
        "parenthesized_expression" | "primary_expression" => children(node)
            .into_iter()
            .find_map(|child| {
                let path = path_segments(child, source);
                (!path.is_empty()).then_some(path)
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn attribute_segments(node: Node<'_>, source: &[u8]) -> Vec<String> {
    let mut path = field(node, "object")
        .map(|object| path_segments(object, source))
        .unwrap_or_default();
    if !path.is_empty()
        && let Some(attribute) = field(node, "attribute")
    {
        path.push(text(attribute, source));
    }
    path
}

pub(super) fn kind_for(node: Node<'_>) -> ReferenceKind {
    if is_type_context(node) {
        ReferenceKind::Type
    } else if matches!(node.kind(), "attribute" | "dotted_name") {
        ReferenceKind::Qualified
    } else {
        ReferenceKind::Value
    }
}

fn is_type_context(node: Node<'_>) -> bool {
    let mut current = node.parent();
    while let Some(item) = current {
        match item.kind() {
            "type" => return true,
            "function_definition" | "class_definition" | "lambda" => return false,
            _ => current = item.parent(),
        }
    }
    false
}

pub(super) fn is_outer_attribute(node: Node<'_>) -> bool {
    node.parent().is_none_or(|parent| {
        parent.kind() != "attribute"
            || field(parent, "object").is_none_or(|object| object.id() != node.id())
    })
}

pub(super) fn is_label(node: Node<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        (parent.kind() == "keyword_argument"
            && field(parent, "name").is_some_and(|name| name.id() == node.id()))
            || (parent.kind() == "keyword_pattern"
                && children(parent)
                    .first()
                    .is_some_and(|name| name.id() == node.id()))
    })
}

pub(super) fn is_attribute_label(node: Node<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        parent.kind() == "attribute"
            && field(parent, "attribute").is_some_and(|attribute| attribute.id() == node.id())
    })
}

pub(super) fn is_store_target(node: Node<'_>) -> bool {
    if !matches!(node.kind(), "attribute" | "dotted_name") {
        return false;
    }
    ancestors(node).any(|parent| {
        matches!(
            parent.kind(),
            "assignment" | "augmented_assignment" | "for_statement" | "for_in_clause"
        ) && field(parent, "left").is_some_and(|target| contains(&range(target), &range(node)))
    })
}

pub(super) fn contains(outer: &std::ops::Range<usize>, inner: &std::ops::Range<usize>) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}
