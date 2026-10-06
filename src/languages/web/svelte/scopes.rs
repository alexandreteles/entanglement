use std::ops::Range;

use tree_sitter::Node;

use super::named_children;

pub(super) fn each_scope(node: Node<'_>) -> Option<Range<usize>> {
    let block = ancestor(node, "each_block")?;
    let end = named_children(block)
        .find(|child| child.kind() == "else_clause")
        .map_or(block.end_byte(), |child| child.start_byte());
    Some(node.end_byte()..end)
}

pub(super) fn await_scope(node: Node<'_>) -> Option<Range<usize>> {
    if let Some(branch) = ancestor(node, "await_branch") {
        return Some(node.end_byte()..branch.end_byte());
    }
    let block = ancestor(node, "await_block")?;
    let end = named_children(block)
        .find(|child| child.kind() == "await_branch" && child.start_byte() > node.end_byte())
        .map_or(block.end_byte(), |child| child.start_byte());
    Some(node.end_byte()..end)
}

pub(super) fn snippet_scope(node: Node<'_>) -> Option<Range<usize>> {
    let block = ancestor(node, "snippet_block")?;
    Some(node.end_byte()..block.end_byte())
}

pub(super) fn declaration_scope(node: Node<'_>, root: &Range<usize>) -> Option<Range<usize>> {
    let scope = lexical_parent_scope(node, root);
    (node.end_byte() < scope.end).then_some(node.end_byte()..scope.end)
}

pub(super) fn lexical_parent_scope(node: Node<'_>, root: &Range<usize>) -> Range<usize> {
    let mut current = node.parent();
    while let Some(item) = current {
        let scope = match item.kind() {
            "snippet_block" | "else_if_clause" | "else_clause" | "await_pending"
            | "await_branch" | "element" | "key_block" | "document" => Some(item.byte_range()),
            "if_block" => Some(block_body_scope(item, &["else_if_clause", "else_clause"])),
            "each_block" => Some(block_body_scope(item, &["else_clause"])),
            "await_block" => Some(block_body_scope(item, &["await_branch"])),
            _ => None,
        };
        if let Some(scope) = scope {
            return scope;
        }
        current = item.parent();
    }
    root.clone()
}

fn block_body_scope(block: Node<'_>, branch_kinds: &[&str]) -> Range<usize> {
    let end = named_children(block)
        .filter(|child| branch_kinds.contains(&child.kind()))
        .map(|child| child.start_byte())
        .min()
        .unwrap_or_else(|| block.end_byte());
    block.start_byte()..end
}

pub(super) fn ancestor<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut current = node.parent();
    while let Some(item) = current {
        if item.kind() == kind {
            return Some(item);
        }
        current = item.parent();
    }
    None
}
