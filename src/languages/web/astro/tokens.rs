use std::ops::Range;

use tree_sitter::Node;

use crate::metrics::halstead::HalsteadToken;

pub(super) fn filter(root: Node<'_>, ignored_before: usize, tokens: &mut Vec<HalsteadToken>) {
    let text_ranges = text_ranges(root);
    tokens.retain(|token| {
        token.start_byte >= ignored_before
            && !text_ranges.iter().any(|range| overlaps(range, token))
    });
}

fn text_ranges(root: Node<'_>) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if matches!(
            node.kind(),
            "jsx_text" | "cdata" | "html_character_reference" | "raw_text"
        ) {
            ranges.push(node.byte_range());
        } else {
            let mut cursor = node.walk();
            pending.extend(node.named_children(&mut cursor));
        }
    }
    ranges
}

fn overlaps(range: &Range<usize>, token: &HalsteadToken) -> bool {
    range.start < token.end_byte && token.start_byte < range.end
}
