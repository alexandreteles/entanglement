use std::collections::HashMap;

use super::super::{SyntaxEvent, SyntaxRole};
use super::{Context, contribution};

impl Context<'_> {
    pub(super) fn logical_contributions(
        &self,
        owned_events: &[&SyntaxEvent],
    ) -> Vec<crate::model::ComplexityContribution> {
        let operators = owned_events
            .iter()
            .copied()
            .filter(|event| {
                matches!(
                    event.role,
                    SyntaxRole::CognitiveLogicalAnd | SyntaxRole::CognitiveLogicalOr
                )
            })
            .filter_map(|event| {
                let expression = event.parent_id?;
                self.logical_expressions
                    .contains(&expression)
                    .then_some((event, expression))
            });
        let mut groups = HashMap::<usize, Vec<&SyntaxEvent>>::new();
        for (operator, expression) in operators {
            let root = logical_root(
                expression,
                &self.logical_expressions,
                &self.parentheses,
                self.parents,
            );
            groups.entry(root).or_default().push(operator);
        }
        let mut result = Vec::new();
        for group in groups.values_mut() {
            group.sort_by_key(|event| event.start_byte);
            append_operator_changes(group, &mut result);
        }
        result
    }
}

fn append_operator_changes(
    group: &[&SyntaxEvent],
    result: &mut Vec<crate::model::ComplexityContribution>,
) {
    let mut previous = None;
    for event in group.iter().copied() {
        if previous != Some(event.role) {
            result.push(contribution(event, "logical_operator", 1));
        }
        previous = Some(event.role);
    }
}

fn logical_root(
    expression: usize,
    logical_expressions: &std::collections::HashSet<usize>,
    parentheses: &std::collections::HashSet<usize>,
    parents: &HashMap<usize, Option<usize>>,
) -> usize {
    let mut current = expression;
    loop {
        let Some(parent) = parents.get(&current).copied().flatten() else {
            return current;
        };
        if logical_expressions.contains(&parent) || parentheses.contains(&parent) {
            current = parent;
        } else {
            return current;
        }
    }
}
