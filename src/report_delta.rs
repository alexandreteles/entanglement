//! Compare metric reports across an in-memory source edit.

use crate::model;

/// Compare function scores and map old decision positions through source edits.
pub(crate) fn function_deltas(
    before: Option<&model::FileAnalysis>,
    after: Option<&model::FileAnalysis>,
    edits: &[tree_sitter::InputEdit],
) -> Vec<model::FunctionDelta> {
    let old_functions = before.map_or(&[][..], |file| file.functions.as_slice());
    let new_functions = after.map_or(&[][..], |file| file.functions.as_slice());
    let mut available: Vec<_> = new_functions.iter().map(Some).collect();
    let mut pairs = Vec::new();
    for old in old_functions {
        let mapped = edited_position(old.start_byte, edits);
        let matches: Vec<_> = available
            .iter()
            .enumerate()
            .filter_map(|(index, new)| {
                new.filter(|new| new.name == old.name)
                    .map(|new| (index, new))
            })
            .collect();
        let index = matches
            .iter()
            .find(|(_, new)| mapped == Some(new.start_byte))
            .map(|(index, _)| *index)
            .or_else(|| {
                (matches.len() == 1
                    && old_functions
                        .iter()
                        .filter(|function| function.name == old.name)
                        .count()
                        == 1)
                    .then(|| matches[0].0)
            });
        pairs.push((Some(old), index.and_then(|index| available[index].take())));
    }
    pairs.extend(available.into_iter().flatten().map(|new| (None, Some(new))));
    pairs
        .into_iter()
        .map(|(old, new)| {
            let old_nloc = old.map_or(0, |function| function.nloc);
            let new_nloc = new.map_or(0, |function| function.nloc);
            let old_cc = old.map_or(0, |function| function.cyclomatic_complexity);
            let new_cc = new.map_or(0, |function| function.cyclomatic_complexity);
            let old_density = old.map_or(0.0, |function| function.cyclomatic_density);
            let new_density = new.map_or(0.0, |function| function.cyclomatic_density);
            let old_cognitive = old.map_or(0, |function| function.cognitive_complexity);
            let new_cognitive = new.map_or(0, |function| function.cognitive_complexity);
            let (added_contributions, removed_contributions) = contribution_changes(
                old.map(|function| function.contributions.as_slice()),
                new.map(|function| function.contributions.as_slice()),
                edits,
            );
            let (added_cognitive_contributions, removed_cognitive_contributions) =
                contribution_changes(
                    old.map(|function| function.cognitive_contributions.as_slice()),
                    new.map(|function| function.cognitive_contributions.as_slice()),
                    edits,
                );
            model::FunctionDelta {
                name: old
                    .or(new)
                    .expect("A function pair has a function")
                    .name
                    .clone(),
                nloc: metric_delta(old_nloc, new_nloc, |before, after| {
                    after as i64 - before as i64
                }),
                cyclomatic_complexity: metric_delta(old_cc, new_cc, |before, after| {
                    after as i64 - before as i64
                }),
                cyclomatic_density: metric_delta(old_density, new_density, |before, after| {
                    after - before
                }),
                cognitive_complexity: metric_delta(
                    old_cognitive,
                    new_cognitive,
                    |before, after| after as i64 - before as i64,
                ),
                added_contributions,
                removed_contributions,
                added_cognitive_contributions,
                removed_cognitive_contributions,
            }
        })
        .collect()
}

/// Keep the before and after values together with their computed change.
fn metric_delta<T: Copy, D>(
    before: T,
    after: T,
    difference: impl FnOnce(T, T) -> D,
) -> model::MetricDelta<T, D> {
    let delta = difference(before, after);
    model::MetricDelta {
        before,
        after,
        delta,
    }
}

/// Compare decisions by role and edited start position; retain matched baselines.
fn contribution_changes(
    before: Option<&[model::ComplexityContribution]>,
    after: Option<&[model::ComplexityContribution]>,
    edits: &[tree_sitter::InputEdit],
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
fn edited_position(position: usize, edits: &[tree_sitter::InputEdit]) -> Option<usize> {
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
