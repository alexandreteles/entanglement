use tree_sitter::{Node, Range, Tree};

pub(crate) fn normalize(ranges: &mut Vec<Range>) {
    ranges.sort_by_key(|range| (range.start_byte, range.end_byte));
    let mut normalized: Vec<Range> = Vec::with_capacity(ranges.len());
    for range in ranges.drain(..) {
        match normalized.last_mut() {
            Some(last) if range.start_byte <= last.end_byte => {
                if range.end_byte > last.end_byte {
                    last.end_byte = range.end_byte;
                    last.end_point = range.end_point;
                }
            }
            _ => normalized.push(range),
        }
    }
    *ranges = normalized;
}

pub(crate) fn intersect_sets(left: &[Range], right: &[Range]) -> Vec<Range> {
    let mut overlaps = Vec::new();
    let (mut left_index, mut right_index) = (0, 0);
    while left_index < left.len() && right_index < right.len() {
        if let Some(overlap) = intersect(left[left_index], right[right_index]) {
            overlaps.push(overlap);
        }
        if left[left_index].end_byte <= right[right_index].end_byte {
            left_index += 1;
        } else {
            right_index += 1;
        }
    }
    normalize(&mut overlaps);
    overlaps
}

pub(super) fn included(tree: &Tree) -> Vec<Range> {
    let ranges = tree.included_ranges();
    if ranges.is_empty() {
        vec![tree.root_node().range()]
    } else {
        ranges
    }
}

pub(super) fn node_segments(node: Node<'_>, ranges: &[Range]) -> Vec<Range> {
    ranges
        .iter()
        .filter_map(|range| intersect(node.range(), *range))
        .collect()
}

pub(super) fn fully_included(node: Node<'_>, ranges: &[Range]) -> bool {
    let node = node.range();
    ranges.iter().any(|range| contains(*range, node))
}

pub(super) fn subtract(content: Range, exclusions: &[Range]) -> Vec<Range> {
    let mut exclusions = exclusions
        .iter()
        .filter_map(|range| intersect(content, *range))
        .collect::<Vec<_>>();
    exclusions.sort_by_key(|range| (range.start_byte, range.end_byte));
    let mut result = Vec::new();
    let mut start_byte = content.start_byte;
    let mut start_point = content.start_point;
    for excluded in exclusions {
        if start_byte < excluded.start_byte {
            result.push(Range {
                start_byte,
                start_point,
                end_byte: excluded.start_byte,
                end_point: excluded.start_point,
            });
        }
        if excluded.end_byte > start_byte {
            start_byte = excluded.end_byte;
            start_point = excluded.end_point;
        }
    }
    if start_byte < content.end_byte {
        result.push(Range {
            start_byte,
            start_point,
            end_byte: content.end_byte,
            end_point: content.end_point,
        });
    }
    result
}

pub(super) fn intersect(left: Range, right: Range) -> Option<Range> {
    let start_byte = left.start_byte.max(right.start_byte);
    let end_byte = left.end_byte.min(right.end_byte);
    (start_byte < end_byte).then_some(Range {
        start_byte,
        start_point: if start_byte == left.start_byte {
            left.start_point
        } else {
            right.start_point
        },
        end_byte,
        end_point: if end_byte == left.end_byte {
            left.end_point
        } else {
            right.end_point
        },
    })
}

fn contains(outer: Range, inner: Range) -> bool {
    outer.start_byte <= inner.start_byte && inner.end_byte <= outer.end_byte
}
