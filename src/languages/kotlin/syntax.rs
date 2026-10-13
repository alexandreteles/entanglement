use std::ops::Range;

use tree_sitter::Node;

use lexer::{
    declaration_after, is_identifier_byte, keyword_at, matching_parenthesis, read_identifier,
    skip_backtick, skip_comment, skip_quoted, skip_trivia, utf8_width,
};

mod events;
mod lexer;
mod recovery;

pub(super) use events::{append_context_terminals, context_terminal_ranges};
pub(super) use events::{append_kotlin_decision_events, remove_else_events_for_else_if};
pub(super) use recovery::{recovered_type_for, recovered_types};

#[derive(Clone, Debug)]
pub(super) struct ContextParameters {
    /// The `context(...)` prefix, including its keyword and closing delimiter.
    pub range: Range<usize>,
    /// First non-trivia byte after the prefix, usually a modifier or annotation.
    pub declaration_start: usize,
    /// The declaration keyword after any recognized modifiers.
    pub declaration_keyword: usize,
    pub parameters: Vec<ContextParameter>,
}

#[derive(Clone, Debug)]
pub(super) struct ContextParameter {
    pub name: Option<String>,
    pub name_range: Option<Range<usize>>,
    pub type_name: String,
    pub type_range: Range<usize>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum RecoveredTypeKind {
    Class,
    Interface,
    Object,
    CompanionObject,
}

#[derive(Clone, Debug)]
pub(super) struct RecoveredType {
    pub error_range: Range<usize>,
    pub declaration_range: Range<usize>,
    pub keyword_range: Range<usize>,
    pub name_range: Option<Range<usize>>,
    pub body_range: Range<usize>,
    pub kind: RecoveredTypeKind,
}

/// Find declaration-prefixed context clauses in Kotlin source.
///
/// This scanner only recognizes `context(...)` when its closing delimiter is
/// followed by a declaration keyword. It skips nested comments, quoted
/// literals, and backtick identifiers, so ordinary calls and string contents
/// remain unchanged.
pub(in crate::languages::kotlin) fn context_parameters(source: &[u8]) -> Vec<ContextParameters> {
    let mut contexts = Vec::new();
    let mut offset = 0;
    while offset < source.len() {
        if let Some(end) = skip_comment(source, offset) {
            offset = end;
            continue;
        }
        if let Some(end) = skip_quoted(source, offset) {
            offset = end;
            continue;
        }
        if source[offset] == b'`' {
            offset = skip_backtick(source, offset);
            continue;
        }
        if keyword_at(source, offset, b"context") {
            let open = skip_trivia(source, offset + b"context".len());
            if source.get(open) == Some(&b'(')
                && (offset == 0 || !is_identifier_byte(source[offset - 1]))
                && let Some(close) = matching_parenthesis(source, open)
                && let Some((declaration_start, declaration_keyword)) =
                    declaration_after(source, close + 1)
            {
                contexts.push(ContextParameters {
                    range: offset..close + 1,
                    declaration_start,
                    declaration_keyword,
                    parameters: parse_context_parameters(source, open + 1..close),
                });
                offset = close + 1;
                continue;
            }
        }
        offset += utf8_width(source[offset]);
    }
    contexts
}

pub(in crate::languages::kotlin) fn context_parameters_at(
    source: &[u8],
    base: usize,
) -> Vec<ContextParameters> {
    let mut contexts = context_parameters(source);
    if base == 0 {
        return contexts;
    }
    for context in &mut contexts {
        context.range.start += base;
        context.range.end += base;
        context.declaration_start += base;
        context.declaration_keyword += base;
        for parameter in &mut context.parameters {
            if let Some(range) = &mut parameter.name_range {
                range.start += base;
                range.end += base;
            }
            parameter.type_range.start += base;
            parameter.type_range.end += base;
        }
    }
    contexts
}

/// Return source with recognized context prefixes replaced by equal-length
/// whitespace. Tree-sitter offsets remain aligned with the user's source.
pub(in crate::languages::kotlin) fn mask_context_parameters(source: &[u8]) -> Option<Vec<u8>> {
    let contexts = context_parameters(source);
    if contexts.is_empty() {
        return None;
    }
    let mut normalized = source.to_vec();
    for context in contexts {
        for byte in &mut normalized[context.range] {
            if !matches!(*byte, b'\n' | b'\r') {
                *byte = b' ';
            }
        }
    }
    Some(normalized)
}

pub(in crate::languages::kotlin) fn context_for_declaration<'a>(
    node: Node<'_>,
    contexts: &'a [ContextParameters],
) -> Option<&'a ContextParameters> {
    contexts.iter().find(|context| {
        context.range.end <= node.start_byte()
            && context.declaration_start <= node.start_byte()
            && node.start_byte() <= context.declaration_keyword
    })
}

pub(in crate::languages::kotlin) fn context_start(
    node: Node<'_>,
    contexts: &[ContextParameters],
) -> usize {
    context_for_declaration(node, contexts).map_or(node.start_byte(), |context| context.range.start)
}

pub(super) fn parse_context_parameters(
    source: &[u8],
    content: Range<usize>,
) -> Vec<ContextParameter> {
    split_top_level(source, content, b',')
        .into_iter()
        .filter_map(|segment| {
            let segment = trim_space(source, segment);
            if segment.is_empty() {
                return None;
            }
            let colon = top_level_colon(source, segment.clone());
            let parsed_name = colon.and_then(|colon| {
                let name_start = skip_trivia(source, segment.start);
                let (name, name_end) = read_identifier(source, name_start)?;
                (skip_trivia(source, name_end) == colon)
                    .then(|| (name.to_owned(), name_start..name_end, colon))
            });
            let (name, name_range, type_range) =
                if let Some((name, name_range, colon)) = parsed_name {
                    (
                        Some(name),
                        Some(name_range),
                        trim_trivia(source, colon + 1..segment.end),
                    )
                } else {
                    (None, None, trim_trivia(source, segment))
                };
            if type_range.is_empty() {
                return None;
            }
            Some(ContextParameter {
                name,
                name_range,
                type_name: text_without_comments(source, type_range.clone()),
                type_range,
            })
        })
        .collect()
}

fn top_level_delimiters(
    source: &[u8],
    range: Range<usize>,
    delimiter: u8,
) -> impl Iterator<Item = usize> + '_ {
    let mut offset = range.start;
    let mut depths = [0usize; 4];
    std::iter::from_fn(move || {
        while offset < range.end {
            if let Some(end) = skip_comment(source, offset)
                .or_else(|| skip_quoted(source, offset))
                .filter(|end| *end <= range.end)
            {
                offset = end;
                continue;
            }
            if source[offset] == b'`' {
                offset = skip_backtick(source, offset).min(range.end);
                continue;
            }
            let current = offset;
            let byte = source[offset];
            offset += utf8_width(byte);
            match byte {
                b'(' => depths[0] += 1,
                b')' => depths[0] = depths[0].saturating_sub(1),
                b'[' => depths[1] += 1,
                b']' => depths[1] = depths[1].saturating_sub(1),
                b'{' => depths[2] += 1,
                b'}' => depths[2] = depths[2].saturating_sub(1),
                b'<' => depths[3] += 1,
                b'>' => depths[3] = depths[3].saturating_sub(1),
                byte if byte == delimiter && depths == [0; 4] => return Some(current),
                _ => {}
            }
        }
        None
    })
}

fn split_top_level(source: &[u8], range: Range<usize>, delimiter: u8) -> Vec<Range<usize>> {
    let mut start = range.start;
    let mut parts = top_level_delimiters(source, range.clone(), delimiter)
        .map(|offset| {
            let part = start..offset;
            start = offset + 1;
            part
        })
        .collect::<Vec<_>>();
    parts.push(start..range.end);
    parts
}

fn top_level_colon(source: &[u8], range: Range<usize>) -> Option<usize> {
    top_level_delimiters(source, range, b':').find(|offset| {
        source.get(offset.wrapping_sub(1)) != Some(&b':') && source.get(offset + 1) != Some(&b':')
    })
}

pub(super) fn trim_space(source: &[u8], mut range: Range<usize>) -> Range<usize> {
    while range.start < range.end && source[range.start].is_ascii_whitespace() {
        range.start += 1;
    }
    while range.start < range.end && source[range.end - 1].is_ascii_whitespace() {
        range.end -= 1;
    }
    range
}

pub(super) fn trim_trivia(source: &[u8], range: Range<usize>) -> Range<usize> {
    let mut offset = range.start;
    let mut last_significant = offset;
    while offset < range.end {
        if source[offset].is_ascii_whitespace() {
            offset += 1;
            continue;
        }
        if let Some(end) = skip_comment(source, offset).filter(|end| *end <= range.end) {
            offset = end;
            continue;
        }
        let end = if let Some(end) = skip_quoted(source, offset).filter(|end| *end <= range.end) {
            end
        } else if source[offset] == b'`' {
            skip_backtick(source, offset).min(range.end)
        } else {
            offset + utf8_width(source[offset])
        };
        offset = end.max(offset + 1);
        last_significant = offset;
    }
    range.start..last_significant
}

pub(super) fn text_without_comments(source: &[u8], range: Range<usize>) -> String {
    let mut text = Vec::with_capacity(range.len());
    let mut offset = range.start;
    while offset < range.end {
        if let Some(end) = skip_quoted(source, offset).filter(|end| *end <= range.end) {
            text.extend_from_slice(&source[offset..end]);
            offset = end;
        } else if source[offset] == b'`' {
            let end = skip_backtick(source, offset).min(range.end);
            text.extend_from_slice(&source[offset..end]);
            offset = end;
        } else if let Some(end) = skip_comment(source, offset).filter(|end| *end <= range.end) {
            text.push(b' ');
            offset = end;
        } else {
            text.push(source[offset]);
            offset += 1;
        }
    }
    String::from_utf8_lossy(&text).into_owned()
}
