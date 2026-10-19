//! Skip Kotlin literals and comments without a syntax tree.

pub(crate) fn skip_comment(source: &[u8], offset: usize) -> Option<usize> {
    if source.get(offset..offset + 2) == Some(b"//") {
        return Some(
            source[offset + 2..]
                .iter()
                .position(|byte| matches!(byte, b'\n' | b'\r'))
                .map_or(source.len(), |length| offset + 2 + length),
        );
    }
    if source.get(offset..offset + 2) != Some(b"/*") {
        return None;
    }
    let mut depth = 1usize;
    let mut end = offset + 2;
    while end < source.len() {
        if source.get(end..end + 2) == Some(b"/*") {
            depth += 1;
            end += 2;
        } else if source.get(end..end + 2) == Some(b"*/") {
            depth -= 1;
            end += 2;
            if depth == 0 {
                return Some(end);
            }
        } else {
            end += utf8_width(source[end]);
        }
    }
    Some(end)
}

pub(super) fn skip_quoted(source: &[u8], offset: usize) -> Option<usize> {
    let quote = *source.get(offset)?;
    if !matches!(quote, b'"' | b'\'') {
        return None;
    }
    let triple = quote == b'"' && source.get(offset..offset + 3) == Some(b"\"\"\"");
    let delimiter_length = if triple { 3 } else { 1 };
    let mut end = offset + delimiter_length;
    while end < source.len() {
        if triple && source.get(end..end + 3) == Some(b"\"\"\"") {
            return Some(end + 3);
        }
        if !triple && source[end] == quote {
            return Some(end + 1);
        }
        if !triple && source[end] == b'\\' {
            end = (end + 2).min(source.len());
        } else {
            end += utf8_width(source[end]);
        }
    }
    Some(end)
}

pub(super) fn skip_backtick(source: &[u8], offset: usize) -> usize {
    let mut end = offset + 1;
    while end < source.len() {
        if source[end] == b'`' {
            return end + 1;
        }
        if matches!(source[end], b'\n' | b'\r') {
            return end;
        }
        end += utf8_width(source[end]);
    }
    end
}

pub(super) fn utf8_width(byte: u8) -> usize {
    match byte {
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => 1,
    }
}
