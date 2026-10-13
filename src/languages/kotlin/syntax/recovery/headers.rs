use tree_sitter::Node;

use super::{RecoveredTypeKind, nodes};

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
        .find(|leaf| leaf.kind() == "{" && nodes::contains_node(*body, *leaf))?;
    let closing = nodes::matching_closing_brace(leaves, opening)?;
    (nodes::contains_node(*body, closing) || body.kind() == "{").then_some((opening, closing))
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
