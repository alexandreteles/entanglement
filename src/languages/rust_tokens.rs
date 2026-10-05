use tree_sitter::Node;

use crate::metrics::halstead::{HalsteadToken, HalsteadTokenKind};

/// Capture Rust lexical tokens from syntax leaves while keeping literals and
/// lifetime names whole and dropping comments entirely.
pub(super) fn capture(root: Node<'_>, source: &[u8]) -> Vec<HalsteadToken> {
    let mut tokens = Vec::new();
    let mut pending = vec![root];

    while let Some(node) = pending.pop() {
        let kind = node.kind();
        if is_ignored(kind) || node.is_missing() {
            continue;
        }

        if is_atomic_operand(kind) || is_identifier(kind) {
            push_token(&mut tokens, node, source, HalsteadTokenKind::Operand);
            continue;
        }

        if node.child_count() == 0 {
            push_token(&mut tokens, node, source, HalsteadTokenKind::Operator);
            continue;
        }

        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                pending.push(child);
            }
        }
    }

    tokens.sort_by_key(|token| (token.start_byte, token.end_byte));
    tokens
}

fn is_ignored(kind: &str) -> bool {
    matches!(kind, "line_comment" | "block_comment" | "shebang")
}

fn is_atomic_operand(kind: &str) -> bool {
    matches!(
        kind,
        "string_literal"
            | "raw_string_literal"
            | "char_literal"
            | "integer_literal"
            | "float_literal"
            | "boolean_literal"
            | "lifetime"
            | "label"
    )
}

fn is_identifier(kind: &str) -> bool {
    matches!(
        kind,
        "identifier"
            | "type_identifier"
            | "field_identifier"
            | "shorthand_field_identifier"
            | "primitive_type"
            | "self"
            | "crate"
            | "super"
            | "metavariable"
    )
}

fn push_token(
    tokens: &mut Vec<HalsteadToken>,
    node: Node<'_>,
    source: &[u8],
    kind: HalsteadTokenKind,
) {
    let range = node.byte_range();
    let Some(token) = source
        .get(range.clone())
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
    else {
        return;
    };
    if token.is_empty() {
        return;
    }
    tokens.push(HalsteadToken {
        kind,
        token: token.to_owned(),
        start_byte: range.start,
        end_byte: range.end,
        line: node.start_position().row + 1,
    });
}

#[cfg(test)]
#[path = "../../tests/unit/rust_tokens.rs"]
mod tests;
