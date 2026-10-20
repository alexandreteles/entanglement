use std::collections::HashMap;

use tree_sitter::Node;

use crate::metrics::{SyntaxEvent, SyntaxRole};

/// Add Kotlin-specific branches that are not explicit in the parse tree.
/// A `when` without `else` may be exhaustive by type information, but that
/// cannot be established from syntax alone. Count a conservative implicit
/// fall-through arm for source-only analysis.
pub(in crate::languages::kotlin) fn append_kotlin_decision_events(
    nodes: &[Node<'_>],
    events: &mut Vec<SyntaxEvent>,
    parents: &mut HashMap<usize, Option<usize>>,
) {
    for node in nodes
        .iter()
        .copied()
        .filter(|node| node.kind() == "when_expression")
    {
        let entries = (0..node.child_count())
            .filter_map(|index| node.child(index))
            .filter(|child| child.kind() == "when_entry")
            .collect::<Vec<_>>();
        let has_else = entries.iter().any(|entry| {
            (0..entry.child_count())
                .filter_map(|index| entry.child(index))
                .any(|child| child.kind() == "else")
        });
        if has_else {
            continue;
        }
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

/// Remove an `else` point when the branch is an `else if`; the nested `if`
/// event already represents that branch in cognitive complexity.
pub(in crate::languages::kotlin) fn remove_else_events_for_else_if(
    nodes: &[Node<'_>],
    events: &mut Vec<SyntaxEvent>,
) {
    let else_if_bodies = nodes
        .iter()
        .filter(|node| {
            node.kind() == "control_structure_body"
                && node
                    .parent()
                    .is_some_and(|parent| parent.kind() == "if_expression")
                && (0..node.named_child_count()).any(|index| {
                    node.named_child(index as u32)
                        .is_some_and(|child| child.kind() == "if_expression")
                })
        })
        .map(|node| node.id())
        .collect::<Vec<_>>();
    events.retain(|event| {
        event.role != SyntaxRole::CognitiveElse || !else_if_bodies.contains(&event.node_id)
    });
}
