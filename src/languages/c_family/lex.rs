use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Word,
    Literal,
    Operator,
    Comment,
}

pub(crate) struct Token {
    pub range: Range<usize>,
    pub kind: Kind,
}

/// Read physical preprocessing tokens, including opaque macro replacement lists.
/// Comments and literals stay intact so directive text inside them has no effect.
pub(crate) fn tokens(source: &[u8]) -> impl Iterator<Item = Token> + '_ {
    let mut position = 0;
    std::iter::from_fn(move || {
        position += source[position..]
            .iter()
            .take_while(|byte| byte.is_ascii_whitespace())
            .count();
        let rest = source.get(position..).filter(|rest| !rest.is_empty())?;
        let (kind, length) = next(rest);
        let start = position;
        position += length;
        Some(Token {
            range: start..position,
            kind,
        })
    })
}

fn next(rest: &[u8]) -> (Kind, usize) {
    if rest.starts_with(b"//") {
        return (Kind::Comment, logical_line_end(rest));
    }
    if rest.starts_with(b"/*") {
        let length = memchr::memmem::find(&rest[2..], b"*/").map_or(rest.len(), |end| end + 4);
        return (Kind::Comment, length);
    }
    if let Some(length) = literal(rest) {
        return (Kind::Literal, length);
    }
    if rest[0].is_ascii_digit() || rest[0] == b'.' && rest.get(1).is_some_and(u8::is_ascii_digit) {
        let mut end = 1;
        while let Some(&byte) = rest.get(end) {
            if !(word(byte)
                || byte == b'.'
                || byte == b'\''
                || matches!(byte, b'+' | b'-')
                    && matches!(rest[end - 1], b'e' | b'E' | b'p' | b'P'))
            {
                break;
            }
            end += 1;
        }
        return (Kind::Literal, end);
    }
    if word(rest[0]) {
        return (
            Kind::Word,
            rest.iter().take_while(|&&byte| word(byte)).count(),
        );
    }
    const OPS: &[&[u8]] = &[
        b"%:%:", b"<=>", b"<<=", b">>=", b"->*", b"...", b"::", b"->", b".*", b"++", b"--", b"&&",
        b"||", b"==", b"!=", b"<=", b">=", b"<<", b">>", b"+=", b"-=", b"*=", b"/=", b"%=", b"&=",
        b"|=", b"^=", b"##", b"<:", b":>", b"<%", b"%>", b"%:",
    ];
    (
        Kind::Operator,
        OPS.iter()
            .find(|op| rest.starts_with(op))
            .map_or(1, |op| op.len()),
    )
}

fn word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

pub(super) fn logical_line_end(source: &[u8]) -> usize {
    memchr::memchr_iter(b'\n', source)
        .find(|&index| {
            let prefix = source[..index]
                .strip_suffix(b"\r")
                .unwrap_or(&source[..index]);
            !prefix.ends_with(b"\\")
        })
        .unwrap_or(source.len())
}

fn literal(rest: &[u8]) -> Option<usize> {
    let prefix = [b"u8".as_slice(), b"u", b"U", b"L"]
        .into_iter()
        .find(|prefix| rest.starts_with(prefix))
        .map_or(0, <[u8]>::len);
    let body = &rest[prefix..];
    if let Some(raw) = body.strip_prefix(b"R\"") {
        let opening = memchr::memchr(b'(', raw)?;
        if opening > 16 {
            return None;
        }
        let closing = [b")".as_slice(), &raw[..opening], b"\""].concat();
        return Some(
            memchr::memmem::find(&raw[opening + 1..], &closing)
                .map_or(rest.len(), |end| prefix + 3 + opening + end + closing.len()),
        );
    }
    let &quote = body.first().filter(|&&byte| matches!(byte, b'\'' | b'"'))?;
    let mut end = prefix + 1;
    while end < rest.len() {
        let byte = rest[end];
        end += 1;
        if byte == quote || byte == b'\n' {
            break;
        }
        if byte == b'\\' {
            end = (end + 1).min(rest.len());
        }
    }
    Some(end)
}
