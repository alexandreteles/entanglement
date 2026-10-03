use std::collections::BTreeSet;
use std::ops::Range;

use super::{SyntaxEvent, SyntaxRole};

/// Return source rows that contain terminal code inside `scope`.
///
/// Treat `scope` and each item in `excluded` as half-open byte ranges. Ignore
/// comments, empty recovery nodes, and syntax inside excluded ranges. Return an
/// empty set when no source row contains code. This function does not return an
/// error.
pub fn rows(
    events: &[SyntaxEvent],
    scope: Range<usize>,
    excluded: &[Range<usize>],
) -> BTreeSet<usize> {
    let comments: Vec<_> = events
        .iter()
        .filter(|event| event.role == SyntaxRole::Comment)
        .map(|event| event.range())
        .collect();

    events
        .iter()
        .filter(|event| {
            event.role == SyntaxRole::Node && event.terminal && event.start_byte < event.end_byte
        })
        .filter(|event| event.start_byte >= scope.start && event.end_byte <= scope.end)
        .filter(|event| !overlaps(event.range(), excluded))
        .filter(|event| {
            !comments
                .iter()
                .any(|comment| contains(comment, event.range()))
        })
        .flat_map(|event| {
            let last_row = if event.end_column == 0 && event.end_row > event.start_row {
                event.end_row - 1
            } else {
                event.end_row
            };
            event.start_row..=last_row
        })
        .collect()
}

fn contains(outer: &Range<usize>, inner: Range<usize>) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

fn overlaps(range: Range<usize>, excluded: &[Range<usize>]) -> bool {
    excluded
        .iter()
        .any(|excluded| range.start < excluded.end && excluded.start < range.end)
}
