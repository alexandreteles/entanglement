use std::ops::Range;

use tree_sitter::Node;

const FUNCTIONS: &[&str] = &["function_declaration", "method_declaration", "func_literal"];

const DECLARATIONS: &[&str] = &[
    "short_var_declaration",
    "range_clause",
    "receive_statement",
    "type_switch_statement",
    "var_spec",
    "const_spec",
    "type_spec",
    "type_alias",
];

/// Explicit blocks, and the statements and clauses that form implicit blocks.
const BLOCKS: &[&str] = &[
    "block",
    "if_statement",
    "for_statement",
    "expression_switch_statement",
    "type_switch_statement",
    "expression_case",
    "type_case",
    "communication_case",
    "default_case",
    "func_literal",
    "function_declaration",
    "method_declaration",
];

pub(super) fn enclosing_function(node: Node<'_>) -> Option<Node<'_>> {
    ancestors(node)
        .skip(1)
        .find(|item| FUNCTIONS.contains(&item.kind()))
}

/// Parameters of function types and interface methods bind no runtime names.
pub(super) fn parameter_scope(name: Node<'_>) -> Option<Range<usize>> {
    ancestors(name)
        .find(|item| {
            FUNCTIONS.contains(&item.kind())
                || matches!(item.kind(), "function_type" | "method_elem")
        })
        .filter(|owner| FUNCTIONS.contains(&owner.kind()))
        .map(|owner| owner.byte_range())
}

/// A local name is visible from the end of its declaration, so `x := x + 1`
/// and `switch x := x.(type)` read the outer `x`. Type names are visible in
/// their own declaration to allow recursive types.
pub(super) fn declaration_scope(name: Node<'_>) -> Option<Range<usize>> {
    let declaration = ancestors(name).find(|item| DECLARATIONS.contains(&item.kind()))?;
    let start = match declaration.kind() {
        "type_switch_statement" => declaration.child_by_field_name("value")?.end_byte(),
        "type_spec" | "type_alias" => declaration.start_byte(),
        _ => declaration.end_byte(),
    };
    let block = ancestors(declaration).find(|item| BLOCKS.contains(&item.kind()))?;
    Some(start..block.end_byte())
}

fn ancestors(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    std::iter::successors(Some(node), Node::parent)
}
