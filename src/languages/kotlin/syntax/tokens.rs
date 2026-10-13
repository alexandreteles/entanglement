use std::ops::Range;

use crate::metrics::{SyntaxEvent, SyntaxRole};

use super::{
    ContextParameters,
    lexer::{
        consume_identifier, consume_number, consume_punctuation, is_identifier_start,
        skip_backtick, skip_comment, skip_quoted,
    },
};

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
