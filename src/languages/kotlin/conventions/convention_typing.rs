use std::ops::Range;

use tree_sitter::Node;

use crate::graph::{BindingType, ReceiverScope};

pub(super) fn expression_type(
    node: Node<'_>,
    source: &[u8],
    bindings: &[BindingType],
    receivers: &[ReceiverScope],
) -> Option<String> {
    if node.kind() == "this_expression" {
        if node_text(node, source).contains('@') {
            return None;
        }
        return receivers
            .iter()
            .filter(|receiver| {
                receiver.start_byte <= node.start_byte() && node.end_byte() <= receiver.end_byte
            })
            .min_by_key(|receiver| receiver.end_byte - receiver.start_byte)
            .map(|receiver| receiver.receiver_type.clone())
            .or_else(|| enclosing_type_name(node, source));
    }
    if node.kind() != "simple_identifier" {
        return None;
    }
    let name = node_text(node, source);
    let name = name.trim_matches('`');
    bindings
        .iter()
        .filter(|binding| {
            binding.name == name
                && binding.name_start_byte <= node.start_byte()
                && binding.scope_start <= node.start_byte()
                && node.end_byte() <= binding.scope_end
        })
        .min_by_key(|binding| {
            (
                binding.scope_end - binding.scope_start,
                usize::MAX - binding.name_start_byte,
            )
        })
        .and_then(|binding| binding.type_name.clone())
}

pub(super) fn simple_identifier(node: Node<'_>, source: &[u8]) -> Option<String> {
    (node.kind() == "simple_identifier")
        .then(|| node_text(node, source).trim_matches('`').to_owned())
}

pub(super) fn lexical_scope(node: Node<'_>, root: Node<'_>) -> Range<usize> {
    ancestors(node)
        .find(|ancestor| {
            matches!(
                ancestor.kind(),
                "statements" | "class_body" | "enum_class_body" | "lambda_literal"
            )
        })
        .map(|scope| scope.byte_range())
        .unwrap_or_else(|| root.byte_range())
}

pub(super) fn enclosing_type_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    let mut owners = ancestors(node)
        .filter(|ancestor| {
            matches!(
                ancestor.kind(),
                "class_declaration" | "object_declaration" | "companion_object"
            )
        })
        .map(|declaration| {
            named_children(declaration)
                .into_iter()
                .find(|child| child.kind() == "type_identifier")
                .map(|name| node_text(name, source).trim_matches('`').to_owned())
                .unwrap_or_else(|| "Companion".to_owned())
        })
        .collect::<Vec<_>>();
    owners.reverse();
    (!owners.is_empty()).then(|| owners.join("."))
}

pub(super) fn is_default_argument(node: Node<'_>, source: &[u8]) -> bool {
    for ancestor in ancestors(node) {
        if matches!(
            ancestor.kind(),
            "function_declaration" | "anonymous_function" | "lambda_literal"
        ) {
            return false;
        }
        if matches!(
            ancestor.kind(),
            "parameter" | "function_value_parameter" | "class_parameter"
        ) && source
            .get(ancestor.byte_range())
            .is_some_and(|parameter| parameter.contains(&b'='))
        {
            return true;
        }
    }
    false
}

pub(super) fn ancestors(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    std::iter::successors(Some(node), |node| node.parent())
}

pub(super) fn all_nodes(root: Node<'_>) -> Vec<Node<'_>> {
    let mut result = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        result.push(node);
        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                pending.push(child);
            }
        }
    }
    result.sort_by_key(|node| (node.start_byte(), node.end_byte()));
    result
}

pub(super) fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    (0..node.named_child_count())
        .filter_map(|index| node.named_child(index as u32))
        .collect()
}

pub(super) fn node_text(node: Node<'_>, source: &[u8]) -> String {
    source
        .get(node.byte_range())
        .map(String::from_utf8_lossy)
        .map(|value| value.into_owned())
        .unwrap_or_default()
}
