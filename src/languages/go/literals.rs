/// Decode a Go string value while retaining the original token for metrics.
/// Byte escapes are assembled before UTF-8 validation; rune escapes reject surrogates.
pub(crate) fn unquote(raw: &str) -> Option<String> {
    if let Some(body) = raw.strip_prefix('`').and_then(|s| s.strip_suffix('`')) {
        return (!body.contains('`')).then(|| body.replace('\r', ""));
    }
    let body = raw.strip_prefix('"')?.strip_suffix('"')?;
    let mut bytes = body.bytes();
    let mut result = Vec::new();
    while let Some(byte) = bytes.next() {
        match byte {
            b'\\' => escape(&mut bytes, &mut result)?,
            b'"' | b'\n' | b'\r' => return None,
            _ => result.push(byte),
        }
    }
    String::from_utf8(result).ok()
}

fn escape(bytes: &mut std::str::Bytes<'_>, output: &mut Vec<u8>) -> Option<()> {
    let first = bytes.next()?;
    if let Some(value) = simple_escape(first) {
        output.push(value);
        return Some(());
    }
    let (value, unicode) = numeric_escape(bytes, first)?;
    if unicode {
        output.extend_from_slice(char::from_u32(value)?.encode_utf8(&mut [0; 4]).as_bytes());
    } else {
        output.push(u8::try_from(value).ok()?);
    }
    Some(())
}

fn numeric_escape(bytes: &mut std::str::Bytes<'_>, first: u8) -> Option<(u32, bool)> {
    let (count, radix, initial, unicode) = match first {
        b'x' => (2, 16, 0, false),
        b'u' => (4, 16, 0, true),
        b'U' => (8, 16, 0, true),
        b'0'..=b'7' => (2, 8, u32::from(first - b'0'), false),
        _ => return None,
    };
    Some((digits(bytes, count, radix, initial)?, unicode))
}

fn simple_escape(byte: u8) -> Option<u8> {
    let names = b"abfnrtv\\\"";
    let values = [7, 8, 12, 10, 13, 9, 11, b'\\', b'"'];
    names
        .iter()
        .position(|name| *name == byte)
        .map(|index| values[index])
}

fn digits(
    bytes: &mut std::str::Bytes<'_>,
    count: usize,
    radix: u32,
    mut value: u32,
) -> Option<u32> {
    for _ in 0..count {
        value = value
            .checked_mul(radix)?
            .checked_add(char::from(bytes.next()?).to_digit(radix)?)?;
    }
    Some(value)
}

#[cfg(test)]
#[path = "../../../tests/unit/go_literals.rs"]
mod tests;
