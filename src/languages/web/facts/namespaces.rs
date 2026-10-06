use std::ops::Range;

use tree_sitter::Node;

use super::scope::{enclosing_function, scope_range, text};

#[derive(Clone)]
pub(super) struct NamespaceReference {
    pub(super) alias: String,
    pub(super) member: String,
    pub(super) range: Range<usize>,
    pub(super) object: Range<usize>,
    pub(super) property: Range<usize>,
    pub(super) scope: Range<usize>,
    pub(super) call_owner: Option<usize>,
    pub(super) is_call: bool,
}

pub(super) fn namespace_reference(
    node: Node<'_>,
    source: &[u8],
    root: Range<usize>,
) -> Option<NamespaceReference> {
    let object = node.child_by_field_name("object")?;
    let property = node.child_by_field_name("property")?;
    if object.kind() != "identifier" || is_member_receiver(node) {
        return None;
    }
    let (scope_start, scope_end) = scope_range(node, root, false);
    Some(NamespaceReference {
        alias: text(object, source),
        member: text(property, source),
        range: node.byte_range(),
        object: object.byte_range(),
        property: property.byte_range(),
        scope: scope_start..scope_end,
        call_owner: enclosing_function(node),
        is_call: is_call_target(node),
    })
}

fn is_member_receiver(node: Node<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        parent.kind() == "member_expression"
            && parent
                .child_by_field_name("object")
                .is_some_and(|receiver| receiver.id() == node.id())
    })
}

fn is_call_target(node: Node<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        parent.kind() == "call_expression"
            && parent
                .child_by_field_name("function")
                .is_some_and(|function| function.id() == node.id())
    })
}
