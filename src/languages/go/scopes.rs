use std::ops::Range;

use tree_sitter::Node;

mod parameters;
pub(super) use parameters::receiver_type_parameters;

#[derive(Clone, Copy)]
pub(super) enum LocalKind {
    Parameter,
    TypeParameter,
    Declaration,
    SwitchAlias,
}

pub(super) fn local_scopes(name: Node<'_>, kind: LocalKind) -> Vec<Range<usize>> {
    let scope = match kind {
        LocalKind::Parameter => parameter_scope(name),
        LocalKind::TypeParameter => parameters::type_parameter_scope(name),
        LocalKind::Declaration => declaration_scope(name),
        LocalKind::SwitchAlias => return parameters::switch_alias_scopes(name),
    };
    scope.into_iter().collect()
}

const FUNCTIONS: &[&str] = &["function_declaration", "method_declaration", "func_literal"];

const DECLARATIONS: &[&str] = &[
    "short_var_declaration",
    "range_clause",
    "receive_statement",
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
        .and_then(|owner| owner.child_by_field_name("body"))
        .map(|body| body.byte_range())
}

/// A local name is visible from the end of its declaration, so `x := x + 1`
/// and `switch x := x.(type)` read the outer `x`. Type names are visible in
/// their own declaration to allow recursive types.
pub(super) fn declaration_scope(name: Node<'_>) -> Option<Range<usize>> {
    let declaration = ancestors(name).find(|item| DECLARATIONS.contains(&item.kind()))?;
    let start = match declaration.kind() {
        "type_spec" | "type_alias" => declaration.start_byte(),
        _ => declaration.end_byte(),
    };
    let block = ancestors(declaration).find(|item| BLOCKS.contains(&item.kind()))?;
    Some(start..block.end_byte())
}

pub(super) fn ancestors(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    std::iter::successors(Some(node), Node::parent)
}
