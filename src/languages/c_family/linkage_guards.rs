use std::ops::Range;

use tree_sitter::{Point, Range as SyntaxRange};

/// Return the source ranges to parse, or nothing when the whole source parses as written.
///
/// A header shared by C and C++ opens `extern "C" {` in one `__cplusplus`
/// conditional and closes it in another. Neither grammar parses a block split
/// across conditionals, and linkage does not change what the declarations
/// inside mean, so the brace lines are left out of the parse.
pub(super) fn ranges_outside_linkage_braces(source: &[u8]) -> Vec<SyntaxRange> {
    let mut ranges = Vec::new();
    let mut range_start = 0;
    for brace_line in linkage_brace_lines(source) {
        if brace_line.start > range_start {
            ranges.push(syntax_range(source, range_start..brace_line.start));
        }
        range_start = brace_line.end;
    }
    if range_start > 0 && range_start < source.len() {
        ranges.push(syntax_range(source, range_start..source.len()));
    }
    ranges
}

fn linkage_brace_lines(source: &[u8]) -> Vec<Range<usize>> {
    let mut brace_lines = Vec::new();
    // Lines of the open `__cplusplus` branch, kept while it holds nothing but linkage braces.
    let mut branch: Option<Vec<Range<usize>>> = None;
    let mut line_start = 0;
    for line in source.split_inclusive(|byte| *byte == b'\n') {
        let line_range = line_start..line_start + line.len();
        line_start = line_range.end;
        let code = code_without_spaces(line);
        if let Some(directive) = code.strip_prefix('#') {
            let finished = branch.take();
            if matches!(directive, "else" | "endif") {
                brace_lines.extend(finished.into_iter().flatten());
            }
            if matches!(
                directive,
                "ifdef__cplusplus" | "ifdefined(__cplusplus)" | "ifdefined__cplusplus"
            ) {
                branch = Some(Vec::new());
            }
        } else if matches!(code.as_str(), "extern\"C\"{" | "extern\"C\"" | "{" | "}") {
            if let Some(lines) = &mut branch {
                lines.push(line_range);
            }
        } else if !code.is_empty() {
            branch = None;
        }
    }
    brace_lines
}

fn code_without_spaces(line: &[u8]) -> String {
    let text = String::from_utf8_lossy(line);
    let code = text.split("//").next().unwrap_or_default();
    let code = code.split("/*").next().unwrap_or_default();
    code.chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn syntax_range(source: &[u8], range: Range<usize>) -> SyntaxRange {
    SyntaxRange {
        start_byte: range.start,
        end_byte: range.end,
        start_point: point(source, range.start),
        end_point: point(source, range.end),
    }
}

fn point(source: &[u8], byte: usize) -> Point {
    let before = &source[..byte];
    let line_start = memchr::memrchr(b'\n', before).map_or(0, |newline| newline + 1);
    Point {
        row: memchr::memchr_iter(b'\n', before).count(),
        column: byte - line_start,
    }
}
