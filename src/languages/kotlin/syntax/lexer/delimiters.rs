use super::*;

pub(in crate::languages::kotlin::syntax) fn matching_parenthesis(
    source: &[u8],
    open: usize,
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
        match source[offset] {
            b'(' => depth += 1,
            b')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(offset);
                }
            }
            _ => {}
        }
        offset += utf8_width(source[offset]);
    }
    None
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
