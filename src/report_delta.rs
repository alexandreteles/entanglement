//! Compare metric reports across an in-memory source edit.

use std::collections::{BTreeMap, HashMap, VecDeque};

use tree_sitter::InputEdit;

use crate::metrics::halstead::{HalsteadMetrics, HalsteadToken, HalsteadTokenKind};
use crate::metrics::selection::{Metric, Selection};
use crate::model;

mod functions;
mod maintainability;

pub(crate) use functions::function_deltas;
pub(crate) use maintainability::compare as maintainability_delta;

type TokenIdentity = (HalsteadTokenKind, String);
type TokenPosition = (HalsteadTokenKind, String, usize);
type TokenIndexes = (
    HashMap<TokenPosition, VecDeque<usize>>,
    HashMap<TokenIdentity, VecDeque<usize>>,
);

/// Compare the whole-file Halstead indicators and token occurrences.
pub(super) fn halstead_delta(
    before: Option<&HalsteadMetrics>,
    after: Option<&HalsteadMetrics>,
    edits: &[InputEdit],
    selection: Selection,
) -> model::HalsteadDelta {
    if !selection.includes(Metric::Halstead) {
        return empty_halstead_delta();
    }
    let zero = HalsteadMetrics::calculate(0, 0, 0, 0);
    let before = before.unwrap_or(&zero);
    let after = after.unwrap_or(&zero);
    let (added_tokens, removed_tokens) = token_changes(before, after, edits);
    model::HalsteadDelta {
        distinct_operators: metric_delta(
            before.distinct_operators,
            after.distinct_operators,
            difference_usize,
        ),
        distinct_operands: metric_delta(
            before.distinct_operands,
            after.distinct_operands,
            difference_usize,
        ),
        total_operators: metric_delta(
            before.total_operators,
            after.total_operators,
            difference_usize,
        ),
        total_operands: metric_delta(
            before.total_operands,
            after.total_operands,
            difference_usize,
        ),
        vocabulary: metric_delta(before.vocabulary, after.vocabulary, difference_usize),
        length: metric_delta(before.length, after.length, difference_usize),
        estimated_length: float_delta(before.estimated_length, after.estimated_length),
        volume: float_delta(before.volume, after.volume),
        difficulty: float_delta(before.difficulty, after.difficulty),
        effort: float_delta(before.effort, after.effort),
        time: float_delta(before.time, after.time),
        program_level: float_delta(before.program_level, after.program_level),
        estimated_bugs: float_delta(before.estimated_bugs, after.estimated_bugs),
        added_tokens,
        removed_tokens,
    }
}

fn empty_halstead_delta() -> model::HalsteadDelta {
    model::HalsteadDelta {
        distinct_operators: metric_delta(0, 0, difference_usize),
        distinct_operands: metric_delta(0, 0, difference_usize),
        total_operators: metric_delta(0, 0, difference_usize),
        total_operands: metric_delta(0, 0, difference_usize),
        vocabulary: metric_delta(0, 0, difference_usize),
        length: metric_delta(0, 0, difference_usize),
        estimated_length: float_delta(0.0, 0.0),
        volume: float_delta(0.0, 0.0),
        difficulty: float_delta(0.0, 0.0),
        effort: float_delta(0.0, 0.0),
        time: float_delta(0.0, 0.0),
        program_level: float_delta(0.0, 0.0),
        estimated_bugs: float_delta(0.0, 0.0),
        added_tokens: Vec::new(),
        removed_tokens: Vec::new(),
    }
}

fn empty_maintainability_delta() -> model::MaintainabilityDelta {
    model::MaintainabilityDelta {
        before: None,
        after: None,
        score: None,
        volume_effect: None,
        cyclomatic_effect: None,
        nloc_effect: None,
        clamp_adjustment: None,
    }
}

/// Compare token identity by kind and spelling, preferring mapped positions.
///
/// Position matching keeps contributors attached to the actual insertion or
/// deletion when a token is repeated. The identity fallback ensures shifts
/// caused by comments or formatting do not appear as token changes.
fn token_changes(
    before: &HalsteadMetrics,
    after: &HalsteadMetrics,
    edits: &[InputEdit],
) -> (
    Vec<model::HalsteadTokenChange>,
    Vec<model::HalsteadTokenChange>,
) {
    let old = tokens(before);
    let new = tokens(after);
    let mut matched_new = vec![false; new.len()];
    let (mut by_position, mut by_identity) = token_indexes(&new);
    let unmatched_old = match_positions(&old, edits, &mut by_position, &mut matched_new);
    let removed = match_identities(unmatched_old, &mut by_identity, &mut matched_new);
    let added = unmatched_tokens(&new, &matched_new);

    (group_tokens(added), group_tokens(removed))
}

fn token_indexes(tokens: &[&HalsteadToken]) -> TokenIndexes {
    let mut positions = HashMap::new();
    let mut identities: HashMap<_, VecDeque<_>> = HashMap::new();
    for (index, token) in tokens.iter().enumerate() {
        positions
            .entry((token.kind, token.token.clone(), token.start_byte))
            .or_insert_with(VecDeque::new)
            .push_back(index);
        identities
            .entry((token.kind, token.token.clone()))
            .or_insert_with(VecDeque::new)
            .push_back(index);
    }
    (positions, identities)
}

fn match_positions<'a>(
    old: &[&'a HalsteadToken],
    edits: &[InputEdit],
    by_position: &mut HashMap<TokenPosition, VecDeque<usize>>,
    matched_new: &mut [bool],
) -> Vec<&'a HalsteadToken> {
    let mut unmatched = Vec::new();
    for &token in old {
        let matched = edited_position(token.start_byte, edits).and_then(|position| {
            by_position
                .get_mut(&(token.kind, token.token.clone(), position))
                .and_then(VecDeque::pop_front)
        });
        if let Some(index) = matched {
            matched_new[index] = true;
        } else {
            unmatched.push(token);
        }
    }
    unmatched
}

fn match_identities<'a>(
    unmatched: Vec<&'a HalsteadToken>,
    by_identity: &mut HashMap<TokenIdentity, VecDeque<usize>>,
    matched_new: &mut [bool],
) -> Vec<&'a HalsteadToken> {
    let mut removed = Vec::new();
    for token in unmatched {
        let key = (token.kind, token.token.clone());
        if let Some(index) = pop_unmatched(by_identity.get_mut(&key), matched_new) {
            matched_new[index] = true;
        } else {
            removed.push(token);
        }
    }
    removed
}

fn pop_unmatched(indices: Option<&mut VecDeque<usize>>, matched: &[bool]) -> Option<usize> {
    let indices = indices?;
    while let Some(index) = indices.pop_front() {
        if !matched[index] {
            return Some(index);
        }
    }
    None
}

fn unmatched_tokens<'a>(tokens: &[&'a HalsteadToken], matched: &[bool]) -> Vec<&'a HalsteadToken> {
    tokens
        .iter()
        .enumerate()
        .filter_map(|(index, &token)| (!matched[index]).then_some(token))
        .collect()
}

fn tokens(metrics: &HalsteadMetrics) -> Vec<&HalsteadToken> {
    metrics.tokens()
}

fn group_tokens(tokens: Vec<&HalsteadToken>) -> Vec<model::HalsteadTokenChange> {
    let mut grouped: Vec<model::HalsteadTokenChange> = Vec::new();
    let mut positions: BTreeMap<TokenPosition, usize> = BTreeMap::new();
    for token in tokens {
        let key = (token.kind, token.token.clone(), token.line);
        if let Some(index) = positions.get(&key) {
            grouped[*index].count += 1;
        } else {
            positions.insert(key, grouped.len());
            grouped.push(model::HalsteadTokenChange {
                kind: token.kind,
                token: token.token.clone(),
                line: token.line,
                count: 1,
            });
        }
    }
    grouped
}

fn metric_delta<T: Copy, D>(
    before: T,
    after: T,
    difference: impl FnOnce(T, T) -> D,
) -> model::MetricDelta<T, D> {
    model::MetricDelta {
        before,
        after,
        delta: difference(before, after),
    }
}

fn difference_usize(before: usize, after: usize) -> i64 {
    after as i64 - before as i64
}

fn float_delta(before: f64, after: f64) -> model::MetricDelta<f64, f64> {
    metric_delta(before, after, |before, after| after - before)
}

fn contribution_changes(
    before: Option<&[model::ComplexityContribution]>,
    after: Option<&[model::ComplexityContribution]>,
    edits: &[InputEdit],
) -> (
    Vec<model::ComplexityContribution>,
    Vec<model::ComplexityContribution>,
) {
    let mut added = after.map_or_else(Vec::new, |contributions| contributions.to_vec());
    let mut removed = Vec::new();
    for old in before.into_iter().flatten() {
        let mapped = edited_position(old.start_byte, edits);
        let index = added.iter().position(|new| {
            new.kind == old.kind
                && new.value == old.value
                && (old.kind == "baseline" || mapped == Some(new.start_byte))
        });
        if let Some(index) = index {
            added.remove(index);
        } else {
            let mut contribution = old.clone();
            contribution.value = -contribution.value;
            removed.push(contribution);
        }
    }
    (added, removed)
}

/// Map a byte position through ordered edits; return none for replaced bytes.
fn edited_position(position: usize, edits: &[InputEdit]) -> Option<usize> {
    edits.iter().try_fold(position, |position, edit| {
        if position < edit.start_byte {
            Some(position)
        } else if position >= edit.old_end_byte {
            position
                .checked_sub(edit.old_end_byte - edit.start_byte)?
                .checked_add(edit.new_end_byte - edit.start_byte)
        } else {
            None
        }
    })
}
