use super::lex::{self, Kind};
use crate::languages::CapturedTree;
use crate::metrics::{
    SyntaxEvent, SyntaxRole,
    halstead::{HalsteadToken, HalsteadTokenKind},
};
use std::ops::Range;

/// Count the tokens written in the file, including macro replacement lists.
/// The syntax tree supplies scope boundaries and runtime decisions.
pub(crate) fn capture(
    source: &[u8],
    ranges: impl IntoIterator<Item = Range<usize>>,
    cpp: bool,
    halstead: bool,
    captured: &mut CapturedTree,
) {
    captured.events.retain(|event| {
        event.role != SyntaxRole::Comment && !(event.role == SyntaxRole::Node && event.terminal)
    });
    captured.tokens.clear();
    let lines: Vec<_> = std::iter::once(0)
        .chain(memchr::memchr_iter(b'\n', source).map(|at| at + 1))
        .collect();
    for range in ranges {
        let mut directive = false;
        for token in lex::tokens(&source[range.clone()]) {
            if token.kind == Kind::Comment {
                continue;
            }
            let start = range.start + token.range.start;
            let end = range.start + token.range.end;
            let row = lines.partition_point(|line| *line <= start) - 1;
            let end_row = lines.partition_point(|line| *line <= end) - 1;
            captured.events.push(SyntaxEvent {
                role: SyntaxRole::Node,
                start_byte: start,
                end_byte: end,
                start_row: row,
                end_row,
                end_column: end - lines[end_row],
                node_id: 0,
                parent_id: None,
                terminal: true,
            });
            if halstead {
                let written = String::from_utf8_lossy(&source[start..end]);
                let operator = token.kind == Kind::Operator
                    || token.kind == Kind::Word && (directive || keyword(&written, cpp));
                directive = written == "#";
                captured.tokens.push(HalsteadToken {
                    kind: if operator {
                        HalsteadTokenKind::Operator
                    } else {
                        HalsteadTokenKind::Operand
                    },
                    token: written.into_owned(),
                    start_byte: start,
                    end_byte: end,
                    line: row + 1,
                });
            }
        }
    }
}

fn keyword(word: &str, cpp: bool) -> bool {
    const COMMON: &str = "alignas alignof auto bool break case char const constexpr continue default do double else enum extern false float for goto if inline int long nullptr register return short signed sizeof static static_assert struct switch thread_local true typedef typeof union unsigned void volatile while";
    const C: &str = "_Alignas _Alignof _Atomic _BitInt _Bool _Complex _Decimal32 _Decimal64 _Decimal128 _Generic _Imaginary _Noreturn _Static_assert _Thread_local restrict typeof_unqual";
    const CPP: &str = "and and_eq asm bitand bitor catch char8_t char16_t char32_t class compl concept const_cast consteval constinit co_await co_return co_yield decltype delete dynamic_cast explicit export friend mutable namespace new noexcept not not_eq operator or or_eq private protected public reinterpret_cast requires static_cast template this throw try typeid typename using virtual wchar_t xor xor_eq override final import module";
    COMMON
        .split_ascii_whitespace()
        .chain(if cpp { CPP } else { C }.split_ascii_whitespace())
        .any(|keyword| keyword == word)
}
