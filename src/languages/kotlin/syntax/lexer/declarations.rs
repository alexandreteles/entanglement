use super::*;

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
    loop {
        if source.get(offset) != Some(&b'.') {
            break;
        }
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
