use std::ops::Range;

use tree_sitter::Node;

use super::super::SemanticFacts;
use super::super::syntax::{ancestors, children, field, range, target_names};
use super::scope::{add_target_bindings, push_local};

pub(super) fn collect_loop_target(
    node: Node<'_>,
    root: Node<'_>,
    source: &[u8],
    facts: &mut SemanticFacts,
    excluded: &mut Vec<Range<usize>>,
) {
    let Some(left) = field(node, "left") else {
        return;
    };
    let Some(comprehension) = nearest_comprehension(node) else {
        add_target_bindings(node, left, root, source, facts, excluded, node.end_byte());
        return;
    };
    let Some(body) = field(comprehension, "body") else {
        return;
    };
    let iterable_end = field(node, "right")
        .map(|right| right.end_byte())
        .unwrap_or_else(|| node.end_byte());
    let first_clause_end = children(comprehension)
        .into_iter()
        .find(|child| child.kind() == "for_in_clause")
        .and_then(|clause| field(clause, "right"))
        .map(|right| right.end_byte())
        .unwrap_or(iterable_end);
    let scopes = [range(body), first_clause_end..comprehension.end_byte()];
    for (name, name_range) in target_names(left, source) {
        excluded.push(name_range.clone());
        for scope in &scopes {
            push_local(facts, name.clone(), name_range.clone(), scope.clone());
        }
    }
}

fn nearest_comprehension(node: Node<'_>) -> Option<Node<'_>> {
    ancestors(node).find(|item| is_comprehension(item.kind()))
}

pub(super) fn is_comprehension(kind: &str) -> bool {
    matches!(
        kind,
        "list_comprehension"
            | "dictionary_comprehension"
            | "set_comprehension"
            | "generator_expression"
    )
}
