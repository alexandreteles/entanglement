//! Kotlin lexical skipping and balanced delimiters.

pub(crate) use super::literals::skip_comment;
pub(super) use super::literals::{skip_backtick, skip_quoted, utf8_width};

pub(in crate::languages::kotlin::syntax) fn skip_trivia(source: &[u8], mut offset: usize) -> usize {
    loop {
        while source.get(offset).is_some_and(u8::is_ascii_whitespace) {
            offset += 1;
        }
        if let Some(end) = skip_comment(source, offset) {
            offset = end;
        } else {
            return offset;
        }
    }
}

pub(in crate::languages::kotlin::syntax) fn declaration_after(
    source: &[u8],
    after: usize,
) -> Option<(usize, usize)> {
    let mut offset = skip_trivia(source, after);
    let declaration_start = offset;
    loop {
        if source.get(offset) == Some(&b'@') {
            offset = skip_annotation(source, offset)?;
            offset = skip_trivia(source, offset);
            continue;
        }
        let (word, end) = read_identifier(source, offset)?;
        if word == "fun" {
            let next = skip_trivia(source, end);
            if read_identifier(source, next).is_some_and(|(next_word, _)| next_word == "interface")
            {
                offset = next;
            } else {
                return Some((declaration_start, offset));
            }
        } else if matches!(word, "enum" | "annotation" | "value") || is_modifier(word) {
            offset = skip_trivia(source, end);
        } else if matches!(
            word,
            "class" | "interface" | "object" | "val" | "var" | "constructor"
        ) {
            return Some((declaration_start, offset));
        } else {
            return None;
        }
    }
}

pub(in crate::languages::kotlin::syntax) fn skip_annotation(
    source: &[u8],
    start: usize,
) -> Option<usize> {
    let mut offset = skip_trivia(source, start + 1);
    let (_, end) = read_identifier(source, offset)?;
    offset = skip_trivia(source, end);
    if source.get(offset) == Some(&b':') {
        offset = skip_trivia(source, offset + 1);
        if source.get(offset) == Some(&b'[') {
            return matching_delimiter(source, offset, b'[', b']').map(|close| close + 1);
        }
        let (_, end) = read_identifier(source, offset)?;
        offset = end;
    }
    while source.get(offset) == Some(&b'.') {
        let (_, end) = read_identifier(source, skip_trivia(source, offset + 1))?;
        offset = end;
    }
    offset = skip_trivia(source, offset);
    if source.get(offset) == Some(&b'(') {
        offset = matching_parenthesis(source, offset)? + 1;
    }
    Some(offset)
}

pub(in crate::languages::kotlin::syntax) fn is_modifier(word: &str) -> bool {
    matches!(
        word,
        "public"
            | "private"
            | "protected"
            | "internal"
            | "expect"
            | "actual"
            | "final"
            | "open"
            | "abstract"
            | "sealed"
            | "const"
            | "external"
            | "override"
            | "lateinit"
            | "tailrec"
            | "suspend"
            | "inline"
            | "noinline"
            | "crossinline"
            | "operator"
            | "infix"
            | "data"
            | "inner"
            | "companion"
            | "reified"
    )
}

pub(in crate::languages::kotlin::syntax) fn matching_parenthesis(
    source: &[u8],
    open: usize,
) -> Option<usize> {
    matching_delimiter(source, open, b'(', b')')
}

pub(in crate::languages::kotlin::syntax) fn matching_delimiter(
    source: &[u8],
    open: usize,
    opening: u8,
    closing: u8,
) -> Option<usize> {
    let mut depth = 0usize;
    let mut offset = open;
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
        if source[offset] == opening {
            depth += 1;
        } else if source[offset] == closing {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(offset);
            }
        }
        offset += utf8_width(source[offset]);
    }
    None
}

pub(in crate::languages::kotlin::syntax) fn keyword_at(
    source: &[u8],
    offset: usize,
    keyword: &[u8],
) -> bool {
    source.get(offset..offset + keyword.len()) == Some(keyword)
        && source
            .get(offset + keyword.len())
            .is_none_or(|byte| !is_identifier_byte(*byte))
}

pub(in crate::languages::kotlin::syntax) fn read_identifier(
    source: &[u8],
    offset: usize,
) -> Option<(&str, usize)> {
    if source.get(offset) == Some(&b'`') {
        let end = skip_backtick(source, offset);
        return std::str::from_utf8(source.get(offset + 1..end.checked_sub(1)?)?)
            .ok()
            .map(|text| (text, end));
    }
    if !source
        .get(offset)
        .is_some_and(|byte| is_identifier_start(*byte))
    {
        return None;
    }
    let end = consume_identifier(source, offset);
    std::str::from_utf8(source.get(offset..end)?)
        .ok()
        .map(|text| (text, end))
}

pub(in crate::languages::kotlin::syntax) fn consume_identifier(
    source: &[u8],
    mut offset: usize,
) -> usize {
    while source
        .get(offset)
        .is_some_and(|byte| is_identifier_byte(*byte))
    {
        offset += utf8_width(source[offset]);
    }
    offset
}

pub(in crate::languages::kotlin::syntax) fn consume_number(
    source: &[u8],
    mut offset: usize,
) -> usize {
    while source
        .get(offset)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'.'))
    {
        offset += 1;
    }
    offset
}

pub(in crate::languages::kotlin::syntax) fn consume_punctuation(
    source: &[u8],
    offset: usize,
) -> usize {
    if matches!(
        source.get(offset..offset + 2),
        Some(b"::" | b"->" | b"?." | b"!!" | b"?:" | b".." | b"<>")
    ) {
        return offset + 2;
    }
    offset + utf8_width(source[offset])
}

pub(in crate::languages::kotlin::syntax) fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

pub(in crate::languages::kotlin::syntax) fn is_identifier_byte(byte: u8) -> bool {
    is_identifier_start(byte) || byte.is_ascii_digit() || byte == b'$'
}
