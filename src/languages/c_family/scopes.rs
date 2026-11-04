use std::ops::Range;

use tree_sitter::Node;

use crate::languages::syntax::ancestors;

/// Statements whose declarations stay visible only inside them.
const BLOCKS: &[&str] = &[
    "compound_statement",
    "for_statement",
    "for_range_loop",
    "if_statement",
    "while_statement",
    "switch_statement",
    "catch_clause",
    "lambda_expression",
    "function_definition",
];

/// A block-scope name is visible from the end of its declarator, so
/// `int x = x;` already reads the new `x`.
pub(super) fn from_name(name: Node<'_>) -> Option<Range<usize>> {
    let block = ancestors(name)
        .skip(1)
        .find(|node| BLOCKS.contains(&node.kind()))?;
    Some(name.end_byte()..block.end_byte())
}
