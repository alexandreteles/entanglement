use std::collections::HashMap;

use tree_sitter::Node;

/// Normalize comprehension clauses into their logical control-flow order.
pub(super) fn normalize_comprehension_parents(
    root: Node<'_>,
    parents: &mut HashMap<usize, Option<usize>>,
) {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if is_comprehension(node.kind()) {
            normalize_comprehension(node, parents);
        }
        let mut cursor = node.walk();
        pending.extend(node.named_children(&mut cursor));
    }
}

fn is_comprehension(kind: &str) -> bool {
    matches!(
        kind,
        "list_comprehension"
            | "set_comprehension"
            | "dictionary_comprehension"
            | "generator_expression"
    )
}

fn normalize_comprehension(comprehension: Node<'_>, parents: &mut HashMap<usize, Option<usize>>) {
    let original_parent = parents.get(&comprehension.id()).copied().flatten();
    let body = comprehension.child_by_field_name("body");
    let mut current_parent = original_parent;
    let mut final_clause = None;
    let mut cursor = comprehension.walk();
    for child in comprehension.named_children(&mut cursor) {
        if Some(child.id()) == body.map(|node| node.id()) {
            continue;
        }
        match child.kind() {
            "for_in_clause" => {
                parents.insert(child.id(), current_parent);
                if let Some(right) = child.child_by_field_name("right") {
                    parents.insert(right.id(), current_parent);
                }
                current_parent = Some(child.id());
                final_clause = current_parent;
            }
            "if_clause" => {
                parents.insert(child.id(), current_parent);
                current_parent = Some(child.id());
                final_clause = current_parent;
            }
            _ => {}
        }
    }
    if let (Some(body), Some(parent)) = (body, final_clause) {
        parents.insert(body.id(), Some(parent));
    }
}
