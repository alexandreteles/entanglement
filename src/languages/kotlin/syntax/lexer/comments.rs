use super::*;

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

pub(in crate::languages::kotlin::syntax) fn skip_comment(
    source: &[u8],
    offset: usize,
) -> Option<usize> {
    if source.get(offset..offset + 2) == Some(b"//") {
        let mut end = offset + 2;
        while end < source.len() && !matches!(source[end], b'\n' | b'\r') {
            end += 1;
        }
        return Some(end);
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

pub(in crate::languages::kotlin::syntax) fn skip_quoted(
    source: &[u8],
    offset: usize,
) -> Option<usize> {
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

pub(in crate::languages::kotlin::syntax) fn skip_backtick(source: &[u8], offset: usize) -> usize {
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
