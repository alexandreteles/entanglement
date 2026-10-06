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
    while let Some(node) = pending.pop() {
        if node.is_missing() {
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
                        kind: if node.is_named() {
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
