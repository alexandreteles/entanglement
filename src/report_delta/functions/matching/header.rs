//! Locate a declaration header without treating quoted or comment text as code.

pub(crate) fn header_end(source: &[u8]) -> usize {
    let mut depth = 0usize;
    let mut index = 0;
    while index < source.len() {
        if let Some(end) = token_end(source, index) {
            index = end;
            continue;
        }
        match source[index] {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth = depth.saturating_sub(1),
            b'{' | b'=' if depth == 0 => return index,
            _ => {}
        }
        index += 1;
    }
    source.len()
}

fn token_end(source: &[u8], index: usize) -> Option<usize> {
    match source[index] {
        b'"' | b'`' => Some(quoted_end(source, index)),
        b'/' if source.get(index + 1) == Some(&b'/') => Some(
            source[index..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(source.len(), |offset| index + offset + 1),
        ),
        b'/' if source.get(index + 1) == Some(&b'*') => Some(comment_end(source, index + 2)),
        _ => None,
    }
}

fn quoted_end(source: &[u8], opening: usize) -> usize {
    let quote = source[opening];
    let mut index = opening + 1;
    while index < source.len() {
        match source[index] {
            byte if byte == quote => return index + 1,
            b'\\' => index += 2,
            _ => index += 1,
        }
    }
    source.len()
}

fn comment_end(source: &[u8], mut index: usize) -> usize {
    let mut depth = 1;
    while index < source.len() && depth > 0 {
        match source.get(index..index + 2) {
            Some(b"/*") => {
                depth += 1;
                index += 2;
            }
            Some(b"*/") => {
                depth -= 1;
                index += 2;
            }
            _ => index += 1,
        }
    }
    index
}
