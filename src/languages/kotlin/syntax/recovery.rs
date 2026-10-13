use tree_sitter::Node;

use super::{RecoveredType, RecoveredTypeKind};

pub(super) mod headers;
pub(super) mod nodes;

pub(in crate::languages::kotlin) fn recovered_types(
    root: Node<'_>,
    source: &[u8],
) -> Vec<RecoveredType> {
    let mut recovered = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if node.kind() == "ERROR" {
            recover_error_types(node, source, &mut recovered);
        }
        nodes::push_children(node, &mut pending);
    }
    recovered.sort_by_key(|item| {
        (
            item.declaration_range.start,
            item.declaration_range.end,
            item.kind,
        )
    });
    recovered.dedup_by(|left, right| {
        left.declaration_range == right.declaration_range && left.kind == right.kind
    });
    recovered
}

pub(in crate::languages::kotlin) fn recovered_type_for<'types>(
    node: Node<'_>,
    recovered: &'types [RecoveredType],
) -> Option<&'types RecoveredType> {
    recovered
        .iter()
        .filter(|item| {
            item.declaration_range.start <= node.start_byte()
                && node.end_byte() <= item.declaration_range.end
        })
        .min_by_key(|item| item.declaration_range.len())
}

fn recover_error_types(error: Node<'_>, source: &[u8], recovered: &mut Vec<RecoveredType>) {
    let children = nodes::direct_children(error);
    let leaves = nodes::leaf_nodes(error);
    for keyword_index in 0..children.len() {
        let Some(header) = headers::header_candidate(&children, keyword_index, source) else {
            continue;
        };
        let Some((opening_brace, closing_brace)) =
            headers::body_braces(&children[header.body_index], &leaves)
        else {
            continue;
        };
        let declaration_range = header.keyword.start_byte()..closing_brace.end_byte();
        let body_range = opening_brace.start_byte()..closing_brace.end_byte();
        if !nodes::range_is_inside(&declaration_range, error.byte_range())
            || body_range.end <= body_range.start
        {
            continue;
        }
        recovered.push(RecoveredType {
            error_range: error.byte_range(),
            declaration_range,
            keyword_range: header.keyword.byte_range(),
            name_range: header.name_range,
            body_range,
            kind: header.kind,
        });
    }
}
