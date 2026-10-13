use std::collections::HashMap;
use std::ops::Range;

use super::{
    ContextParameters,
    lexer::{
        consume_identifier, consume_number, consume_punctuation, is_identifier_start,
        skip_backtick, skip_comment, skip_quoted,
    },
};

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
    for &node in nodes {
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
    }
}

/// Remove an `else` point when the branch is an `else if`; the nested `if`
/// event already represents that branch in cognitive complexity.
pub(in crate::languages::kotlin) fn remove_else_events_for_else_if(
    nodes: &[Node<'_>],
    events: &mut Vec<SyntaxEvent>,
) {
    let mut else_if_bodies = Vec::new();
    for &node in nodes {
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
    }
    events.retain(|event| {
        event.role != SyntaxRole::CognitiveElse || !else_if_bodies.contains(&event.node_id)
    });
}

/// Add syntax terminals omitted by context-prefix masking. NLOC therefore
/// still sees code on context declaration lines, while comment-only lines in
/// a multiline clause remain excluded.
pub(in crate::languages::kotlin) fn append_context_terminals(
    source: &[u8],
    contexts: &[ContextParameters],
    events: &mut Vec<SyntaxEvent>,
) {
    for context in contexts {
        for range in context_terminal_ranges(source, context) {
            let (start_row, _) = source_position(source, range.start);
            let (end_row, end_column) = source_position(source, range.end);
            events.push(SyntaxEvent {
                role: SyntaxRole::Node,
                start_byte: range.start,
                end_byte: range.end,
                start_row,
                end_row,
                end_column,
                node_id: range.start,
                parent_id: None,
                terminal: true,
            });
        }
    }
}

pub(in crate::languages::kotlin) fn context_terminal_ranges(
    source: &[u8],
    context: &ContextParameters,
) -> Vec<Range<usize>> {
    let mut result = Vec::new();
    let mut offset = context.range.start;
    while offset < context.range.end {
        if source[offset].is_ascii_whitespace() {
            offset += 1;
            continue;
        }
        if let Some(end) = skip_comment(source, offset).filter(|end| *end <= context.range.end) {
            offset = end;
            continue;
        }
        let end = if let Some(end) = skip_quoted(source, offset) {
            end.min(context.range.end)
        } else if source[offset] == b'`' {
            skip_backtick(source, offset).min(context.range.end)
        } else if is_identifier_start(source[offset]) {
            consume_identifier(source, offset).min(context.range.end)
        } else if source[offset].is_ascii_digit() {
            consume_number(source, offset).min(context.range.end)
        } else {
            consume_punctuation(source, offset).min(context.range.end)
        };
        if end <= offset {
            offset += 1;
        } else {
            result.push(offset..end);
            offset = end;
        }
    }
    result
}

pub(super) fn source_position(source: &[u8], offset: usize) -> (usize, usize) {
    let before = &source[..offset.min(source.len())];
    let row = before.iter().filter(|byte| **byte == b'\n').count();
    let column = before
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(before.len(), |newline| before.len() - newline - 1);
    (row, column)
}
