use std::ops::Range;

use tree_sitter::Node;

use super::super::syntax::{ancestors, field, range};
use super::paths::contains;

pub(super) fn containing_scope(node: Node<'_>, root: Node<'_>) -> Range<usize> {
    let reference = range(node);
    ancestors(node)
        .find_map(|owner| {
            let body = match owner.kind() {
                "function_definition" | "class_definition" | "lambda" => field(owner, "body"),
                "list_comprehension"
                | "dictionary_comprehension"
                | "set_comprehension"
                | "generator_expression" => Some(owner),
                _ => None,
            }?;
            let scope = range(body);
            contains(&scope, &reference).then_some(scope)
        })
        .unwrap_or_else(|| range(root))
}

pub(super) fn call_owner(call: Node<'_>, target: &Range<usize>) -> Option<usize> {
    let mut current = Some(call);
    while let Some(node) = current {
        if matches!(node.kind(), "function_definition" | "lambda")
            && field(node, "body").is_some_and(|body| contains(&range(body), target))
        {
            return Some(node.start_byte());
        }
        current = node.parent();
    }
    None
}
