use std::ops::Range;

use super::super::CaptureFacts;
use crate::model::ReferenceKind;

impl CaptureFacts<'_> {
    pub(super) fn call_owner(
        &self,
        node_id: usize,
        reference_range: &Range<usize>,
        kind: ReferenceKind,
    ) -> Option<usize> {
        if !matches!(
            kind,
            ReferenceKind::Call
                | ReferenceKind::Method
                | ReferenceKind::Qualified
                | ReferenceKind::Value
        ) {
            return None;
        }
        let reference = self.graph.nodes.get(&node_id).copied()?;
        let call = direct_call(reference, reference_range)?;
        enclosing_function(call)
    }
}

fn direct_call<'tree>(
    reference: tree_sitter::Node<'tree>,
    reference_range: &Range<usize>,
) -> Option<tree_sitter::Node<'tree>> {
    let mut current = reference;
    loop {
        match current.kind() {
            "closure_expression" | "async_block" | "function_item" => return None,
            "call_expression" => {
                let is_direct_target = current
                    .child_by_field_name("function")
                    .is_some_and(|callee| direct_callee(callee, reference, reference_range));
                return is_direct_target.then_some(current);
            }
            _ => current = current.parent()?,
        }
    }
}

fn enclosing_function<'tree>(call: tree_sitter::Node<'tree>) -> Option<usize> {
    let mut current = call.parent()?;
    loop {
        match current.kind() {
            "closure_expression" | "async_block" => return None,
            "function_item" => return Some(current.start_byte()),
            _ => current = current.parent()?,
        }
    }
}

fn direct_callee<'tree>(
    mut callee: tree_sitter::Node<'tree>,
    reference: tree_sitter::Node<'tree>,
    reference_range: &Range<usize>,
) -> bool {
    loop {
        if same_reference(callee, reference, reference_range) {
            return true;
        }
        if let Some(inner) = wrapped_callee(callee) {
            callee = inner;
            continue;
        }
        return terminal_callee_matches(callee, reference, reference_range);
    }
}

fn wrapped_callee<'tree>(callee: tree_sitter::Node<'tree>) -> Option<tree_sitter::Node<'tree>> {
    match callee.kind() {
        "generic_function" => callee.child_by_field_name("function"),
        "parenthesized_expression" if callee.named_child_count() == 1 => callee.named_child(0),
        _ => None,
    }
}

fn terminal_callee_matches(
    callee: tree_sitter::Node<'_>,
    reference: tree_sitter::Node<'_>,
    reference_range: &Range<usize>,
) -> bool {
    let field = match callee.kind() {
        "scoped_identifier" => "name",
        "field_expression" => "field",
        _ => return false,
    };
    callee
        .child_by_field_name(field)
        .is_some_and(|name| same_reference(name, reference, reference_range))
}

fn same_reference(
    node: tree_sitter::Node<'_>,
    reference: tree_sitter::Node<'_>,
    reference_range: &Range<usize>,
) -> bool {
    node.id() == reference.id()
        && node.start_byte() == reference_range.start
        && node.end_byte() == reference_range.end
}
