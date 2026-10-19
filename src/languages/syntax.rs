//! Tree operations shared by language adapters.

use std::ops::Range;
use tree_sitter::Node;

/// Read node text, replacing invalid UTF-8 bytes with the replacement character.
/// The source must contain the node's byte range.
pub(super) fn text(node: Node<'_>, source: &[u8]) -> String {
    String::from_utf8_lossy(&source[node.byte_range()]).into_owned()
}

/// Test whether the outer byte range contains the complete inner range.
pub(super) fn contains(outer: &Range<usize>, inner: &Range<usize>) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

/// Visit the node and then its ancestors, from nearest to farthest.
pub(super) fn ancestors(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    std::iter::successors(Some(node), Node::parent)
}

/// Visit named children in source order without allocating a collection.
pub(super) fn named_children(node: Node<'_>) -> impl DoubleEndedIterator<Item = Node<'_>> {
    (0..node.named_child_count()).filter_map(move |index| node.named_child(index as u32))
}

/// Collect named children for adapters that need indexing or a traversal stack.
pub(super) fn children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}
