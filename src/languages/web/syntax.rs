use tree_sitter::Node;

pub(super) fn normalize_module_specifier(raw: &str) -> Option<String> {
    let quoted = raw.as_bytes();
    if quoted.len() < 2
        || !matches!(quoted[0], b'\'' | b'"')
        || quoted[0] != quoted[quoted.len() - 1]
    {
        return None;
    }
    Some(raw[1..raw.len() - 1].to_owned())
}

pub(super) fn normalize_string_node(node: Node<'_>, source: &[u8]) -> Option<String> {
    normalize_module_specifier(std::str::from_utf8(node_text_bytes(node, source)?).ok()?)
}

pub(super) fn node_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    if node.kind() == "string" {
        normalize_string_node(node, source)
    } else {
        node_text(node, source)
    }
}

pub(super) fn node_text(node: Node<'_>, source: &[u8]) -> Option<String> {
    String::from_utf8(node_text_bytes(node, source)?.to_vec()).ok()
}

fn node_text_bytes<'a>(node: Node<'_>, source: &'a [u8]) -> Option<&'a [u8]> {
    source.get(node.start_byte()..node.end_byte())
}

pub(super) fn named_child<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    (0..node.named_child_count())
        .filter_map(|index| named_child_at(node, index))
        .find(|child| child.kind() == kind)
}

pub(super) fn named_child_at<'tree>(node: Node<'tree>, index: usize) -> Option<Node<'tree>> {
    u32::try_from(index)
        .ok()
        .and_then(|index| node.named_child(index))
}

pub(super) fn has_default_keyword(statement: Node<'_>) -> bool {
    (0..statement.child_count())
        .filter_map(|index| statement.child(index))
        .any(|child| child.kind() == "default")
}

pub(super) fn has_wildcard(statement: Node<'_>) -> bool {
    (0..statement.child_count())
        .filter_map(|index| statement.child(index))
        .any(|child| child.kind() == "*")
}
