use tree_sitter::Node;

use super::{RecoveredType, RecoveredTypeKind};

pub(in crate::languages::kotlin) fn recovered_types(
    root: Node<'_>,
    source: &[u8],
) -> Vec<RecoveredType> {
    let mut recovered = Vec::new();
    // Keep preorder so duplicate recovery candidates retain the same ERROR
    // owner when nested nodes cover the same declaration range.
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if node.kind() == "ERROR" {
            recover_error_types(node, source, &mut recovered);
        }
        push_children(node, &mut pending);
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
    let children = direct_children(error);
    let leaves = leaf_nodes(error);
    for keyword_index in 0..children.len() {
        let Some(header) = header_candidate(&children, keyword_index, source) else {
            continue;
        };
        let Some((opening_brace, closing_brace)) =
            body_braces(&children[header.body_index], &leaves)
        else {
            continue;
        };
        let declaration_range = header.keyword.start_byte()..closing_brace.end_byte();
        let body_range = opening_brace.start_byte()..closing_brace.end_byte();
        if !range_is_inside(&declaration_range, error.byte_range())
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

pub(super) struct HeaderCandidate<'tree> {
    pub(super) kind: RecoveredTypeKind,
    pub(super) keyword: Node<'tree>,
    pub(super) body_index: usize,
    pub(super) name_range: Option<std::ops::Range<usize>>,
}
pub(super) fn header_candidate<'tree>(
    children: &[Node<'tree>],
    keyword_index: usize,
    source: &[u8],
) -> Option<HeaderCandidate<'tree>> {
    let candidate = children[keyword_index];
    let (kind, keyword_index, header_end) = match candidate.kind() {
        "class" => (RecoveredTypeKind::Class, keyword_index, keyword_index),
        "interface" => (RecoveredTypeKind::Interface, keyword_index, keyword_index),
        "object" if !is_companion_object_token(children, keyword_index) => {
            (RecoveredTypeKind::Object, keyword_index, keyword_index)
        }
        "companion" => {
            let object_index = next_non_comment(children, keyword_index + 1)?;
            (
                RecoveredTypeKind::CompanionObject,
                keyword_index,
                object_index,
            )
        }
        _ => return None,
    };
    let body_index =
        (header_end + 1..children.len()).find(|index| is_body_node(children[*index].kind()))?;
    if !valid_header(children, header_end, body_index) {
        return None;
    }
    let name_range = children[header_end + 1..body_index]
        .iter()
        .find(|child| is_name_token(**child) && has_source_text(**child, source))
        .map(Node::byte_range);
    if requires_name(kind) && name_range.is_none() {
        return None;
    }
    Some(HeaderCandidate {
        kind,
        keyword: children[keyword_index],
        body_index,
        name_range,
    })
}
pub(super) fn body_braces<'tree>(
    body: &Node<'tree>,
    leaves: &[Node<'tree>],
) -> Option<(Node<'tree>, Node<'tree>)> {
    if !is_body_node(body.kind()) {
        return None;
    }
    let opening = leaves
        .iter()
        .copied()
        .find(|leaf| leaf.kind() == "{" && contains_node(*body, *leaf))?;
    let closing = matching_closing_brace(leaves, opening)?;
    (contains_node(*body, closing) || body.kind() == "{").then_some((opening, closing))
}
pub(super) fn valid_header(children: &[Node<'_>], header_end: usize, body_index: usize) -> bool {
    header_end < body_index
        && children[header_end + 1..body_index]
            .iter()
            .all(|child| is_allowed_header_child(*child))
}
pub(super) fn is_allowed_header_child(child: Node<'_>) -> bool {
    if is_comment(child.kind()) {
        return true;
    }
    if child.is_named() {
        return matches!(
            child.kind(),
            "type_identifier"
                | "simple_identifier"
                | "identifier"
                | "type_parameters"
                | "primary_constructor"
                | "type_constraints"
                | "delegation_specifier"
                | "delegation_specifiers"
                | "modifiers"
                | "annotation"
        );
    }
    matches!(child.kind(), ":" | "," | "." | "where")
}
pub(super) fn is_companion_object_token(children: &[Node<'_>], object_index: usize) -> bool {
    object_index > 0
        && previous_non_comment(children, object_index)
            .is_some_and(|index| children[index].kind() == "companion")
}
pub(super) fn next_non_comment(children: &[Node<'_>], start: usize) -> Option<usize> {
    (start..children.len()).find(|index| !is_comment(children[*index].kind()))
}
pub(super) fn previous_non_comment(children: &[Node<'_>], before: usize) -> Option<usize> {
    (0..before)
        .rev()
        .find(|index| !is_comment(children[*index].kind()))
}
pub(super) fn is_body_node(kind: &str) -> bool {
    matches!(kind, "{" | "class_body" | "enum_class_body")
}
pub(super) fn is_name_token(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "type_identifier" | "simple_identifier" | "identifier"
    )
}
pub(super) fn has_source_text(node: Node<'_>, source: &[u8]) -> bool {
    source
        .get(node.byte_range())
        .is_some_and(|bytes| !bytes.is_empty())
}
pub(super) fn requires_name(kind: RecoveredTypeKind) -> bool {
    matches!(
        kind,
        RecoveredTypeKind::Class | RecoveredTypeKind::Interface | RecoveredTypeKind::Object
    )
}
pub(super) fn is_comment(kind: &str) -> bool {
    matches!(kind, "line_comment" | "multiline_comment")
}

pub(super) fn push_children<'tree>(node: Node<'tree>, pending: &mut Vec<Node<'tree>>) {
    for index in (0..node.child_count()).rev() {
        if let Some(child) = node.child(index) {
            pending.push(child);
        }
    }
}
pub(super) fn direct_children<'tree>(node: Node<'tree>) -> Vec<Node<'tree>> {
    (0..node.child_count())
        .filter_map(|index| node.child(index))
        .collect()
}
pub(super) fn matching_closing_brace<'tree>(
    leaves: &[Node<'tree>],
    opening: Node<'tree>,
) -> Option<Node<'tree>> {
    let mut depth = 0usize;
    for leaf in leaves
        .iter()
        .copied()
        .filter(|leaf| leaf.start_byte() >= opening.start_byte())
        .filter(|leaf| matches!(leaf.kind(), "{" | "}"))
    {
        if leaf.kind() == "{" {
            depth += 1;
            continue;
        }
        depth = depth.checked_sub(1)?;
        if depth == 0 {
            return Some(leaf);
        }
    }
    None
}
pub(super) fn leaf_nodes(root: Node<'_>) -> Vec<Node<'_>> {
    let mut leaves = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if is_opaque_brace_container(node.kind()) {
            continue;
        }
        if node.child_count() == 0 {
            if !node.is_missing() {
                leaves.push(node);
            }
        } else {
            push_children(node, &mut pending);
        }
    }
    leaves.sort_by_key(|leaf| (leaf.start_byte(), leaf.end_byte()));
    leaves
}
pub(super) fn is_opaque_brace_container(kind: &str) -> bool {
    kind.contains("string")
        || matches!(
            kind,
            "character_literal" | "line_comment" | "multiline_comment"
        )
}
pub(super) fn contains_node(outer: Node<'_>, inner: Node<'_>) -> bool {
    outer.start_byte() <= inner.start_byte() && inner.end_byte() <= outer.end_byte()
}
pub(super) fn range_is_inside(
    inner: &std::ops::Range<usize>,
    outer: std::ops::Range<usize>,
) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}
