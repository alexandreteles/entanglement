use std::ops::Range;

use tree_sitter::Node;

use super::scope::{enclosing_function, scope_range, text};
use crate::model::{ModulePath, Reference, ReferenceKind};

pub(super) struct NamespaceReference {
    pub(super) reference: Reference,
    pub(super) object: Range<usize>,
    pub(super) property: Range<usize>,
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
    let call_owner = enclosing_function(node);
    Some(NamespaceReference {
        reference: Reference {
            path: vec![text(object, source), text(property, source)],
            module: ModulePath::default(),
            kind: if is_call_target(node) {
                ReferenceKind::Call
            } else {
                ReferenceKind::Qualified
            },
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
            scope_start,
            scope_end,
            call_owner,
            execution_owner: call_owner,
            access: None,
            context_id: 0,
        },
        object: object.byte_range(),
        property: property.byte_range(),
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
