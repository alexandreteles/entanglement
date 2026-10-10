use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;

use tree_sitter::{InputEdit, Range as TsRange, Tree};

use crate::languages::query::ranges;
use crate::languages::{CapturedTree, InjectionRequest};

use super::{ParsedInjection, TreeKey};

pub(super) fn remap_node_ids(captured: &mut CapturedTree, next_id: &mut usize) {
    let mut old_ids = BTreeSet::new();
    old_ids.extend(captured.parents.keys().copied());
    old_ids.extend(captured.parents.values().flatten().copied());
    for event in &captured.events {
        old_ids.insert(event.node_id);
        old_ids.extend(event.parent_id);
    }
    let mapping = old_ids
        .into_iter()
        .map(|old| {
            let new = *next_id;
            *next_id += 1;
            (old, new)
        })
        .collect::<HashMap<_, _>>();
    for event in &mut captured.events {
        event.node_id = mapping[&event.node_id];
        event.parent_id = event.parent_id.map(|id| mapping[&id]);
    }
    captured.parents = captured
        .parents
        .drain()
        .map(|(node, parent)| (mapping[&node], parent.map(|id| mapping[&id])))
        .collect();
}

pub(super) fn injection_ranges(tree: &Tree, request: &InjectionRequest) -> Vec<TsRange> {
    let ranges = request.guest_ranges.clone();
    let parent_ranges = tree.included_ranges();
    let parent_ranges = if parent_ranges.is_empty() {
        vec![tree.root_node().range()]
    } else {
        parent_ranges
    };
    let mut ranges = ranges
        .into_iter()
        .flat_map(|range| {
            parent_ranges
                .iter()
                .filter_map(move |parent| intersect(range, *parent))
        })
        .collect::<Vec<_>>();
    ranges::normalize(&mut ranges);
    ranges
}

fn intersect(left: TsRange, right: TsRange) -> Option<TsRange> {
    let start_byte = left.start_byte.max(right.start_byte);
    let end_byte = left.end_byte.min(right.end_byte);
    if start_byte >= end_byte {
        return None;
    }
    let start_point = if start_byte == left.start_byte {
        left.start_point
    } else {
        right.start_point
    };
    let end_point = if end_byte == left.end_byte {
        left.end_point
    } else {
        right.end_point
    };
    Some(TsRange {
        start_byte,
        start_point,
        end_byte,
        end_point,
    })
}

pub(super) fn is_smaller(parent: &Range<usize>, child: &Range<usize>) -> bool {
    parent.start <= child.start
        && child.end <= parent.end
        && child.start < child.end
        && child.end - child.start < parent.end - parent.start
}

pub(in crate::analysis) fn collect_old_trees(
    injections: &[ParsedInjection],
    edits: &[InputEdit],
    trees: &mut BTreeMap<TreeKey, Tree>,
) {
    for injection in injections {
        if let (Some(language), Some(tree), Some(range)) = (
            injection.language_id.as_ref(),
            injection.tree.as_ref(),
            map_range(injection.range.clone(), edits),
        ) {
            let edited = edit_tree(tree.clone(), edits);
            trees
                .entry((language.clone(), range.start, range.end))
                .or_insert(edited);
        }
        collect_old_trees(&injection.children, edits, trees);
    }
}

pub(in crate::analysis) fn edit_tree(mut tree: Tree, edits: &[InputEdit]) -> Tree {
    for edit in edits {
        tree.edit(edit);
    }
    tree
}

fn map_range(mut range: Range<usize>, edits: &[InputEdit]) -> Option<Range<usize>> {
    for edit in edits {
        if edit.old_end_byte <= range.start {
            range.start = shift(range.start, edit.old_end_byte, edit.new_end_byte)?;
            range.end = shift(range.end, edit.old_end_byte, edit.new_end_byte)?;
        } else if edit.start_byte >= range.end {
            continue;
        } else if range.start <= edit.start_byte && edit.old_end_byte <= range.end {
            range.end = shift(range.end, edit.old_end_byte, edit.new_end_byte)?;
        } else {
            return None;
        }
    }
    Some(range)
}

fn shift(value: usize, old_end: usize, new_end: usize) -> Option<usize> {
    if new_end >= old_end {
        value.checked_add(new_end - old_end)
    } else {
        value.checked_sub(old_end - new_end)
    }
}
