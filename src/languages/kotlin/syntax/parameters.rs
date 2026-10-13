use std::ops::Range;

use super::{
    ContextParameter,
    lexer::{read_identifier, skip_backtick, skip_comment, skip_quoted, skip_trivia, utf8_width},
};

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

pub(super) fn split_top_level(
    source: &[u8],
    range: Range<usize>,
    delimiter: u8,
) -> Vec<Range<usize>> {
    let mut parts = Vec::new();
    let mut start = range.start;
    let mut offset = range.start;
    let mut parens = 0usize;
    let mut brackets = 0usize;
    let mut braces = 0usize;
    let mut angles = 0usize;
    while offset < range.end {
        if let Some(end) = skip_comment(source, offset).filter(|end| *end <= range.end) {
            offset = end;
            continue;
        }
        if let Some(end) = skip_quoted(source, offset).filter(|end| *end <= range.end) {
            offset = end;
            continue;
        }
        if source[offset] == b'`' {
            offset = skip_backtick(source, offset).min(range.end);
            continue;
        }
        match source[offset] {
            b'(' => parens += 1,
            b')' => parens = parens.saturating_sub(1),
            b'[' => brackets += 1,
            b']' => brackets = brackets.saturating_sub(1),
            b'{' => braces += 1,
            b'}' => braces = braces.saturating_sub(1),
            b'<' => angles += 1,
            b'>' => angles = angles.saturating_sub(1),
            byte if byte == delimiter
                && parens == 0
                && brackets == 0
                && braces == 0
                && angles == 0 =>
            {
                parts.push(start..offset);
                start = offset + 1;
            }
            _ => {}
        }
        offset += utf8_width(source[offset]);
    }
    parts.push(start..range.end);
    parts
}

pub(super) fn top_level_colon(source: &[u8], range: Range<usize>) -> Option<usize> {
    let mut offset = range.start;
    let mut parens = 0usize;
    let mut brackets = 0usize;
    let mut braces = 0usize;
    let mut angles = 0usize;
    while offset < range.end {
        if let Some(end) = skip_comment(source, offset).filter(|end| *end <= range.end) {
            offset = end;
            continue;
        }
        if let Some(end) = skip_quoted(source, offset).filter(|end| *end <= range.end) {
            offset = end;
            continue;
        }
        if source[offset] == b'`' {
            offset = skip_backtick(source, offset).min(range.end);
            continue;
        }
        match source[offset] {
            b'(' => parens += 1,
            b')' => parens = parens.saturating_sub(1),
            b'[' => brackets += 1,
            b']' => brackets = brackets.saturating_sub(1),
            b'{' => braces += 1,
            b'}' => braces = braces.saturating_sub(1),
            b'<' => angles += 1,
            b'>' => angles = angles.saturating_sub(1),
            b':' if parens == 0
                && brackets == 0
                && braces == 0
                && angles == 0
                && source.get(offset.wrapping_sub(1)) != Some(&b':')
                && source.get(offset + 1) != Some(&b':') =>
            {
                return Some(offset);
            }
            _ => {}
        }
        offset += utf8_width(source[offset]);
    }
    None
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
