use super::lex::{self, Kind};
use std::ops::Range;

#[derive(Clone, Debug)]
pub(crate) struct Directive {
    pub range: Range<usize>,
    pub name: String,
    pub argument: String,
}

/// Find directives at the start of a logical source line. Keep the full physical
/// range for diagnostics when a directive continues onto another line.
pub(crate) fn read(source: &[u8]) -> Vec<Directive> {
    let mut result = Vec::new();
    let mut last_end = 0;
    let mut skip = 0;
    for token in lex::tokens(source).filter(|token| token.kind != Kind::Comment) {
        let start = token.range.start;
        if start < skip {
            continue;
        }
        let first = last_end == 0 || source[last_end..start].contains(&b'\n');
        last_end = token.range.end;
        if !first || &source[token.range] != b"#" {
            continue;
        }
        skip = start + lex::logical_line_end(&source[start..]);
        let line = String::from_utf8_lossy(&source[start + 1..skip])
            .replace("\\\r\n", "")
            .replace("\\\n", "");
        let line = line.trim_start();
        let end = line
            .find(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
            .unwrap_or(line.len());
        result.push(Directive {
            range: start..skip,
            name: line[..end].into(),
            argument: line[end..].trim().into(),
        });
    }
    result
}
