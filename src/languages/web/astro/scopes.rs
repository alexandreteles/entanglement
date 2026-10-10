use std::ops::Range;

use tree_sitter::Node;

pub(super) fn lexical_scope(node: Node<'_>, root: &Range<usize>) -> Range<usize> {
    ancestor(node, is_lexical_scope)
        .map(|scope| scope.byte_range())
        .unwrap_or_else(|| root.clone())
}

pub(super) fn call_owner(node: Node<'_>, root: &Range<usize>) -> usize {
    ancestor(node, is_function).map_or(root.start, |function| function.start_byte())
}

pub(super) fn ancestor<'tree>(
    node: Node<'tree>,
    predicate: impl Fn(Node<'_>) -> bool,
) -> Option<Node<'tree>> {
    let mut current = node.parent();
    while let Some(item) = current {
        if predicate(item) {
            return Some(item);
        }
        current = item.parent();
    }
    None
}

pub(super) fn named_children(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .collect::<Vec<_>>()
        .into_iter()
}

fn is_lexical_scope(node: Node<'_>) -> bool {
    is_function(node)
        || matches!(
            node.kind(),
            "program" | "statement_block" | "switch_body" | "catch_clause" | "class_body"
        )
}

fn is_function(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "function_declaration"
            | "function_expression"
            | "generator_function_declaration"
            | "generator_function"
            | "arrow_function"
            | "method_definition"
    )
}
