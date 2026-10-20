use tree_sitter::Node;

use super::super::node_text;

pub(super) fn path_segments(node: Node<'_>, source: &[u8]) -> Vec<String> {
    if let (Some(path), Some(name)) = (
        node.child_by_field_name("path"),
        node.child_by_field_name("name"),
    ) {
        let mut segments = path_segments(path, source);
        segments.push(node_text(name, source));
        return segments;
    }
    if is_path_node(node) {
        return vec![node_text(node, source)];
    }
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| is_path_node(*child))
        .flat_map(|child| path_segments(child, source))
        .collect()
}

pub(super) fn is_use_node(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "scoped_use_list"
            | "use_as_clause"
            | "use_list"
            | "use_wildcard"
            | "scoped_identifier"
            | "identifier"
            | "crate"
            | "self"
            | "super"
    )
}

pub(super) fn is_path_node(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "scoped_identifier"
            | "scoped_type_identifier"
            | "identifier"
            | "type_identifier"
            | "crate"
            | "self"
            | "super"
    )
}
