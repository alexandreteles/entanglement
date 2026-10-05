//! Compare function deltas while respecting the selected output metrics.

use tree_sitter::InputEdit;

use crate::metrics::selection::{Metric, Selection};
use crate::model;

use super::{
    contribution_changes, edited_position, empty_maintainability_delta, halstead_delta,
    maintainability_delta, metric_delta,
};

/// Compare function scores and contributions before and after source edits.
pub(crate) fn function_deltas(
    before: Option<&model::FileAnalysis>,
    after: Option<&model::FileAnalysis>,
    edits: &[InputEdit],
    selection: Selection,
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
            let (added_contributions, removed_contributions) = if selection.includes(Metric::Cc) {
                contribution_changes(
                    old.map(|function| function.contributions.as_slice()),
                    new.map(|function| function.contributions.as_slice()),
                    edits,
                )
            } else {
                (Vec::new(), Vec::new())
            };
            let (added_cognitive_contributions, removed_cognitive_contributions) =
                if selection.includes(Metric::Cogc) {
                    contribution_changes(
                        old.map(|function| function.cognitive_contributions.as_slice()),
                        new.map(|function| function.cognitive_contributions.as_slice()),
                        edits,
                    )
                } else {
                    (Vec::new(), Vec::new())
                };
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
                maintainability_index: if selection.needs_mi() {
                    maintainability_delta(
                        old.and_then(|function| function.maintainability_index.as_ref()),
                        new.and_then(|function| function.maintainability_index.as_ref()),
                    )
                } else {
                    empty_maintainability_delta()
                },
                halstead: halstead_delta(
                    old.map(|function| &function.halstead),
                    new.map(|function| &function.halstead),
                    edits,
                    selection,
                ),
                added_contributions,
                removed_contributions,
                added_cognitive_contributions,
                removed_cognitive_contributions,
            }
        })
        .collect()
}
