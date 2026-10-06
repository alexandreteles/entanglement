mod comprehensions;
mod declarations;
mod patterns;
mod scope;

use std::ops::Range;

use tree_sitter::Node;

use super::SemanticFacts;
use super::syntax::{field, is_top_level, nodes};

pub(super) use scope::class_segments;

pub(super) fn collect(
    root: Node<'_>,
    source: &[u8],
    facts: &mut SemanticFacts,
) -> Vec<Range<usize>> {
    let mut excluded = Vec::new();
    for node in nodes(root) {
        match node.kind() {
            "assignment" | "augmented_assignment" => {
                collect_assignment(node, root, source, facts, &mut excluded);
            }
            "function_definition" => {
                patterns::bind_parameters(node, source, facts, &mut excluded);
                declarations::collect_definition(node, root, source, facts, &mut excluded);
            }
            "class_definition" => {
                declarations::collect_definition(node, root, source, facts, &mut excluded);
            }
            "lambda" => patterns::bind_lambda_parameters(node, source, facts, &mut excluded),
            "for_statement" | "for_in_clause" => {
                comprehensions::collect_loop_target(node, root, source, facts, &mut excluded);
            }
            "with_item" => {
                declarations::collect_with_target(node, root, source, facts, &mut excluded);
            }
            "except_clause" => {
                patterns::collect_exception_target(node, root, source, facts, &mut excluded);
            }
            "named_expression" => {
                declarations::collect_named_expression(node, root, source, facts, &mut excluded);
            }
            "case_clause" => {
                patterns::collect_case_targets(node, root, source, facts, &mut excluded);
            }
            "global_statement" | "nonlocal_statement" => {
                declarations::collect_scope_declaration(node, root, source, facts, &mut excluded);
            }
            "delete_statement" => {
                declarations::collect_deletion(node, root, source, facts, &mut excluded);
            }
            _ => {}
        }
    }
    excluded
}

fn collect_assignment(
    node: Node<'_>,
    root: Node<'_>,
    source: &[u8],
    facts: &mut SemanticFacts,
    excluded: &mut Vec<Range<usize>>,
) {
    if is_top_level(node) {
        return;
    }
    if let Some(left) = field(node, "left") {
        scope::add_target_bindings(node, left, root, source, facts, excluded, node.end_byte());
    }
}
