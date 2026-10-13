use tree_sitter::Node;

pub(super) fn push_children<'tree>(node: Node<'tree>, pending: &mut Vec<Node<'tree>>) {
    for index in (0..node.child_count()).rev() {
        if let Some(child) = node.child(index) {
            pending.push(child);
        }
    }
}
pub(super) fn direct_children<'tree>(node: Node<'tree>) -> Vec<Node<'tree>> {
    (0..node.child_count())
        .filter_map(|index| node.child(index))
        .collect()
}
pub(super) fn matching_closing_brace<'tree>(
    leaves: &[Node<'tree>],
    opening: Node<'tree>,
) -> Option<Node<'tree>> {
    let mut depth = 0usize;
    for leaf in leaves
        .iter()
        .copied()
        .filter(|leaf| leaf.start_byte() >= opening.start_byte())
        .filter(|leaf| matches!(leaf.kind(), "{" | "}"))
    {
        if leaf.kind() == "{" {
            depth += 1;
            continue;
        }
        depth = depth.checked_sub(1)?;
        if depth == 0 {
            return Some(leaf);
        }
    }
    None
}
pub(super) fn leaf_nodes(root: Node<'_>) -> Vec<Node<'_>> {
    let mut leaves = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if is_opaque_brace_container(node.kind()) {
            continue;
        }
        if node.child_count() == 0 {
            if !node.is_missing() {
                leaves.push(node);
            }
        } else {
            push_children(node, &mut pending);
        }
    }
    leaves.sort_by_key(|leaf| (leaf.start_byte(), leaf.end_byte()));
    leaves
}
pub(super) fn is_opaque_brace_container(kind: &str) -> bool {
    kind.contains("string")
        || matches!(
            kind,
            "character_literal" | "line_comment" | "multiline_comment"
        )
}
pub(super) fn contains_node(outer: Node<'_>, inner: Node<'_>) -> bool {
    outer.start_byte() <= inner.start_byte() && inner.end_byte() <= outer.end_byte()
}
pub(super) fn range_is_inside(
    inner: &std::ops::Range<usize>,
    outer: std::ops::Range<usize>,
) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}
