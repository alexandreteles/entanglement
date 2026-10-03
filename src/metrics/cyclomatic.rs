use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

use crate::model::ComplexityContribution;

use super::{SyntaxEvent, SyntaxRole};

/// A function range used by complexity analysis.
#[derive(Debug, Clone)]
pub struct FunctionScope {
    /// The function name as it appears in source.
    pub name: String,
    /// The source byte range for the full function item.
    pub range: Range<usize>,
    /// The one-based source line for the function start.
    pub line: usize,
}

/// Calculate complexity and list its source contributions for one function.
///
/// `events` must include the function's syntax events. `parents` maps each
/// captured syntax node to its parent. The result has a baseline score of one;
/// it adds each decision event in the function and each extra match arm. An
/// empty event list returns a score of one with the baseline contribution.
pub fn analyze(
    function: &FunctionScope,
    events: &[SyntaxEvent],
    parents: &HashMap<usize, Option<usize>>,
) -> (usize, Vec<ComplexityContribution>) {
    let mut contributions = vec![ComplexityContribution {
        kind: "baseline".into(),
        value: 1,
        start_byte: function.range.start,
        end_byte: function.range.start,
        line: function.line,
    }];

    let multiways: HashSet<_> = events
        .iter()
        .filter(|event| event.role == SyntaxRole::Multiway)
        .map(|event| event.node_id)
        .collect();
    let mut cases = HashMap::<usize, usize>::new();

    for event in events.iter().filter(|event| event.role == SyntaxRole::Case) {
        if let Some(multiway) = nearest_ancestor(event.node_id, parents, &multiways) {
            *cases.entry(multiway).or_default() += 1;
        }
    }

    for event in events.iter().filter(|event| {
        matches!(
            event.role,
            SyntaxRole::Condition | SyntaxRole::LogicalCondition | SyntaxRole::Multiway
        )
    }) {
        if event.start_byte < function.range.start || event.end_byte > function.range.end {
            continue;
        }

        let (kind, value) = match event.role {
            SyntaxRole::Condition => ("condition", 1),
            SyntaxRole::LogicalCondition => ("logical_condition", 1),
            SyntaxRole::Multiway => (
                "multiway",
                cases
                    .get(&event.node_id)
                    .copied()
                    .unwrap_or(0)
                    .saturating_sub(1) as i32,
            ),
            _ => continue,
        };
        if value > 0 {
            contributions.push(ComplexityContribution {
                kind: kind.into(),
                value,
                start_byte: event.start_byte,
                end_byte: event.end_byte,
                line: event.start_row + 1,
            });
        }
    }

    contributions.sort_by_key(|item| (item.start_byte, item.kind.clone()));
    let score = contributions
        .iter()
        .map(|item| item.value.max(0) as usize)
        .sum();
    (score, contributions)
}

fn nearest_ancestor(
    node_id: usize,
    parents: &HashMap<usize, Option<usize>>,
    targets: &HashSet<usize>,
) -> Option<usize> {
    let mut current = parents.get(&node_id).copied().flatten();
    let mut visited = HashSet::new();
    while let Some(node) = current {
        if !visited.insert(node) {
            return None;
        }
        if targets.contains(&node) {
            return Some(node);
        }
        current = parents.get(&node).copied().flatten();
    }
    None
}

/// Assign each decision event to its smallest containing function.
///
/// Return a map from the index in `functions` to that function's decision
/// events. Ignore events outside all function ranges. An event inside nested
/// functions belongs to the smallest containing range.
pub fn assign_events(
    functions: &[FunctionScope],
    events: &[SyntaxEvent],
) -> BTreeMap<usize, Vec<SyntaxEvent>> {
    let mut assigned = BTreeMap::<usize, Vec<SyntaxEvent>>::new();
    for event in events.iter().filter(|event| {
        matches!(
            event.role,
            SyntaxRole::Condition
                | SyntaxRole::LogicalCondition
                | SyntaxRole::Multiway
                | SyntaxRole::Case
        )
    }) {
        if let Some((index, _)) = functions
            .iter()
            .enumerate()
            .filter(|(_, function)| {
                function.range.start <= event.start_byte && event.end_byte <= function.range.end
            })
            .min_by_key(|(_, function)| function.range.end - function.range.start)
        {
            assigned.entry(index).or_default().push(*event);
        }
    }
    assigned
}
