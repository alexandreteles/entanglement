use tree_sitter::Node;

use super::{
    ContextParameters,
    lexer::{
        declaration_after, is_identifier_byte, keyword_at, matching_parenthesis, skip_backtick,
        skip_comment, skip_quoted, skip_trivia, utf8_width,
    },
    parameters::parse_context_parameters,
};

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
