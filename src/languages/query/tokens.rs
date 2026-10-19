use tree_sitter::Node;

use crate::metrics::halstead::{HalsteadToken, HalsteadTokenKind};

use super::ranges::node_segments;

pub(super) fn capture(
    root: Node<'_>,
    source: &[u8],
    included: &[tree_sitter::Range],
) -> Vec<HalsteadToken> {
    let mut tokens = Vec::new();
    let mut pending = vec![root];
    let has_errors = root.has_error();
    while let Some(node) = pending.pop() {
        if node.is_missing()
            || matches!(node.kind(), "jsx_text" | "html_character_reference")
            || (has_errors && node.child_count() == 0 && recovered_jsx_prose(node, source))
        {
            continue;
        }
        if node.child_count() == 0 {
            for range in node_segments(node, included) {
                if let Some(token) = source
                    .get(range.start_byte..range.end_byte)
                    .and_then(|text| std::str::from_utf8(text).ok())
                    .filter(|token| !token.is_empty())
                {
                    tokens.push(HalsteadToken {
                        kind: if node.is_named() || recovered_jsx_attribute(node) {
                            HalsteadTokenKind::Operand
                        } else {
                            HalsteadTokenKind::Operator
                        },
                        token: token.to_owned(),
                        start_byte: range.start_byte,
                        end_byte: range.end_byte,
                        line: range.start_point.row + 1,
                    });
                }
            }
        } else {
            for index in (0..node.child_count()).rev() {
                if let Some(child) = node.child(index) {
                    pending.push(child);
                }
            }
        }
    }
    tokens.sort_by_key(|item| (item.start_byte, item.end_byte));
    tokens
}

/// Ignore bare-ampersand prose recovered directly under JSX markup.
/// Keep expression and nested element boundaries intact.
/// Remove when tree-sitter-javascript#366 is fixed in the pinned grammar.
pub(super) fn recovered_jsx_prose(mut node: Node<'_>, source: &[u8]) -> bool {
    loop {
        if matches!(
            node.kind(),
            "jsx_expression" | "jsx_element" | "jsx_self_closing_element"
        ) {
            return false;
        }
        let Some(parent) = node.parent() else {
            return false;
        };
        if node.is_error()
            && parent.kind() == "jsx_element"
            && source.get(node.start_byte()) == Some(&b'&')
        {
            return true;
        }
        node = parent;
    }
}

/// Keep the reserved JSX attribute `in` in the operand class (#387 upstream).
fn recovered_jsx_attribute(node: Node<'_>) -> bool {
    node.kind() == "in"
        && node
            .parent()
            .filter(|parent| parent.is_error())
            .and_then(|parent| parent.parent())
            .is_some_and(|parent| {
                matches!(
                    parent.kind(),
                    "jsx_opening_element" | "jsx_self_closing_element"
                )
            })
}
