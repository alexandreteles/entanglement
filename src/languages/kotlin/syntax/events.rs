use std::collections::HashMap;

use tree_sitter::Node;

use crate::metrics::{SyntaxEvent, SyntaxRole};

/// Add Kotlin-specific branches that are not explicit in the parse tree.
/// A `when` without `else` may be exhaustive by type information, but that
/// cannot be established from syntax alone. Count a conservative implicit
/// fall-through arm for source-only analysis.
pub(in crate::languages::kotlin) fn append_kotlin_decision_events(
    root: Node<'_>,
    events: &mut Vec<SyntaxEvent>,
    parents: &mut HashMap<usize, Option<usize>>,
) {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if node.kind() == "when_expression" {
            let entries = (0..node.child_count())
                .filter_map(|index| node.child(index))
                .filter(|child| child.kind() == "when_entry")
                .collect::<Vec<_>>();
            let has_else = entries.iter().any(|entry| {
                (0..entry.child_count())
                    .filter_map(|index| entry.child(index))
                    .any(|child| child.kind() == "else")
            });
            if !has_else {
                if let Some(entry) = entries.first() {
                    parents.insert(entry.id(), Some(node.id()));
                    let position = node.end_position();
                    events.push(SyntaxEvent {
                        role: SyntaxRole::Case,
                        start_byte: node.end_byte(),
                        end_byte: node.end_byte(),
                        start_row: position.row,
                        end_row: position.row,
                        end_column: position.column,
                        node_id: entry.id(),
                        parent_id: Some(node.id()),
                        terminal: true,
                    });
                } else {
                    let start = node.start_position();
                    let end = node.end_position();
                    events.push(SyntaxEvent {
                        role: SyntaxRole::Condition,
                        start_byte: node.start_byte(),
                        end_byte: node.end_byte(),
                        start_row: start.row,
                        end_row: end.row,
                        end_column: end.column,
                        node_id: node.id(),
                        parent_id: node.parent().map(|parent| parent.id()),
                        terminal: false,
                    });
                }
            }
        }
        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                pending.push(child);
            }
        }
    }
}

/// Remove an `else` point when the branch is an `else if`; the nested `if`
/// event already represents that branch in cognitive complexity.
pub(in crate::languages::kotlin) fn remove_else_events_for_else_if(
    root: Node<'_>,
    events: &mut Vec<SyntaxEvent>,
) {
    let mut pending = vec![root];
    let mut else_if_bodies = Vec::new();
    while let Some(node) = pending.pop() {
        if node.kind() == "control_structure_body"
            && node
                .parent()
                .is_some_and(|parent| parent.kind() == "if_expression")
            && (0..node.named_child_count()).any(|index| {
                node.named_child(index as u32)
                    .is_some_and(|child| child.kind() == "if_expression")
            })
        {
            else_if_bodies.push(node.id());
        }
        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                pending.push(child);
            }
        }
    }
    events.retain(|event| {
        event.role != SyntaxRole::CognitiveElse || !else_if_bodies.contains(&event.node_id)
    });
}
