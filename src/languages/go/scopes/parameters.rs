use std::ops::Range;

use tree_sitter::Node;

use super::{FUNCTIONS, ancestors};

/// Type parameters include the signature, unlike ordinary value parameters.
pub(super) fn type_parameter_scope(name: Node<'_>) -> Option<Range<usize>> {
    ancestors(name)
        .find(|owner| {
            FUNCTIONS.contains(&owner.kind()) || matches!(owner.kind(), "type_spec" | "type_alias")
        })
        .map(|owner| owner.byte_range())
}

/// Receiver type arguments declare parameters; the receiver's base type does not.
pub(in crate::languages::go) fn receiver_type_parameters(receiver: Node<'_>) -> Vec<Node<'_>> {
    let mut pending = vec![receiver];
    let mut names = Vec::new();
    while let Some(node) = pending.pop() {
        let mut cursor = node.walk();
        if node.kind() == "type_arguments" {
            names.extend(
                node.named_children(&mut cursor)
                    .filter_map(type_parameter_name),
            );
        } else {
            pending.extend(node.named_children(&mut cursor));
        }
    }
    names
}

fn type_parameter_name(node: Node<'_>) -> Option<Node<'_>> {
    let name = node.named_child(0)?;
    (name.kind() == "type_identifier").then_some(name)
}

/// A type-switch alias starts after each case's type list, not in the guard.
pub(super) fn switch_alias_scopes(name: Node<'_>) -> Vec<Range<usize>> {
    let Some(switch) = ancestors(name).find(|node| node.kind() == "type_switch_statement") else {
        return Vec::new();
    };
    let mut cursor = switch.walk();
    switch
        .named_children(&mut cursor)
        .filter(|node| matches!(node.kind(), "type_case" | "default_case"))
        .filter_map(clause_body)
        .collect()
}

fn clause_body(clause: Node<'_>) -> Option<Range<usize>> {
    let mut cursor = clause.walk();
    let colon = clause
        .children(&mut cursor)
        .find(|node| node.kind() == ":")?;
    Some(colon.end_byte()..clause.end_byte())
}
