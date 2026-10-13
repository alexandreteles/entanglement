use super::*;

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
    for symbol in [b"::".as_slice(), b"->", b"?.", b"!!", b"?:", b"..", b"<>"] {
        if source.get(offset..offset + symbol.len()) == Some(symbol) {
            return offset + symbol.len();
        }
    }
    offset + utf8_width(source[offset])
}

pub(in crate::languages::kotlin::syntax) fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

pub(in crate::languages::kotlin::syntax) fn is_identifier_byte(byte: u8) -> bool {
    is_identifier_start(byte) || byte.is_ascii_digit() || byte == b'$'
}

pub(in crate::languages::kotlin::syntax) fn utf8_width(byte: u8) -> usize {
    if byte < 0x80 {
        1
    } else if byte & 0xe0 == 0xc0 {
        2
    } else if byte & 0xf0 == 0xe0 {
        3
    } else if byte & 0xf8 == 0xf0 {
        4
    } else {
        1
    }
}
