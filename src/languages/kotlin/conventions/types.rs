use super::*;

pub(super) fn expression_type(
    node: Node<'_>,
    source: &[u8],
    bindings: &[LocalBinding],
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
        .filter(|binding| binding.evidence != BindingEvidence::Lexical)
        .filter(|binding| {
            binding.name == name
                && binding.start_byte <= node.start_byte()
                && binding.scope_start <= node.start_byte()
                && node.end_byte() <= binding.scope_end
        })
        .min_by_key(|binding| {
            (
                binding.scope_end - binding.scope_start,
                usize::MAX - binding.start_byte,
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

pub(super) fn node_text(node: Node<'_>, source: &[u8]) -> String {
    source
        .get(node.byte_range())
        .map(String::from_utf8_lossy)
        .map(|value| value.into_owned())
        .unwrap_or_default()
}
