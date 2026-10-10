mod indexing;
mod logical;

use std::collections::{HashMap, HashSet};

use crate::model::ComplexityContribution;

use super::{SyntaxEvent, SyntaxRole, cyclomatic::FunctionScope};

/// Return true when the metric needs a syntax role.
///
/// Function and closure events affect nesting, even when they add no points.
pub fn is_event(role: SyntaxRole) -> bool {
    matches!(
        role,
        SyntaxRole::Function
            | SyntaxRole::CognitiveIf
            | SyntaxRole::CognitiveConditionBoundary
            | SyntaxRole::CognitiveElse
            | SyntaxRole::CognitiveElseIf
            | SyntaxRole::CognitiveLoop
            | SyntaxRole::CognitiveLetElse
            | SyntaxRole::CognitiveMultiway
            | SyntaxRole::CognitiveLogicalExpression
            | SyntaxRole::CognitiveLogicalAnd
            | SyntaxRole::CognitiveLogicalOr
            | SyntaxRole::CognitiveParentheses
            | SyntaxRole::CognitiveClosure
            | SyntaxRole::CognitiveLabeledJump
    )
}

/// Keep indexes for one syntax tree and analyze its functions.
///
/// Build one context per tree. Then call [`Context::analyze_functions`] to
/// reuse its indexes across all function metrics.
pub struct Context<'a> {
    events: &'a [SyntaxEvent],
    parents: &'a HashMap<usize, Option<usize>>,
    roles: HashMap<usize, SyntaxRole>,
    function_ranges_by_node: HashMap<usize, std::ops::Range<usize>>,
    else_if_nodes: HashSet<usize>,
    else_if_parent: HashMap<usize, usize>,
    condition_parent: HashMap<usize, usize>,
    logical_expressions: HashSet<usize>,
    parentheses: HashSet<usize>,
}

impl Context<'_> {
    /// Calculate metrics for all functions, in input order.
    pub fn analyze_functions(
        &self,
        functions: &[FunctionScope],
    ) -> Vec<(usize, Vec<ComplexityContribution>)> {
        let mut owned = vec![Vec::<&SyntaxEvent>::new(); functions.len()];
        let mut function_order = (0..functions.len()).collect::<Vec<_>>();
        function_order.sort_by_key(|index| {
            (
                functions[*index].range.start,
                std::cmp::Reverse(functions[*index].range.end),
            )
        });
        let mut direct_events = self
            .events
            .iter()
            .filter(|event| is_scored_event(event.role))
            .collect::<Vec<_>>();
        direct_events.sort_by_key(|event| (event.start_byte, event.end_byte));

        let mut active = Vec::<usize>::new();
        let mut next_function = 0;
        for event in direct_events {
            while next_function < function_order.len()
                && functions[function_order[next_function]].range.start <= event.start_byte
            {
                let index = function_order[next_function];
                while active.last().is_some_and(|active_index| {
                    functions[*active_index].range.end <= functions[index].range.start
                }) {
                    active.pop();
                }
                active.push(index);
                next_function += 1;
            }
            while active
                .last()
                .is_some_and(|index| !contains_range(&functions[*index].range, &event.range()))
            {
                active.pop();
            }
            if let Some(index) = active.last().copied() {
                owned[index].push(event);
            }
        }

        functions
            .iter()
            .zip(owned)
            .map(|(function, events)| self.analyze_owned(function, &events))
            .collect()
    }

    fn analyze_owned(
        &self,
        function: &FunctionScope,
        owned_events: &[&SyntaxEvent],
    ) -> (usize, Vec<ComplexityContribution>) {
        let mut contributions = Vec::new();
        for event in owned_events {
            let (kind, value) = match event.role {
                SyntaxRole::CognitiveIf if self.else_if_nodes.contains(&event.node_id) => ("if", 1),
                SyntaxRole::CognitiveIf => ("if", 1 + self.nesting(event, function)),
                SyntaxRole::CognitiveElse => ("else", 1),
                SyntaxRole::CognitiveLoop => ("loop", 1 + self.nesting(event, function)),
                SyntaxRole::CognitiveLetElse => ("let_else", 1 + self.nesting(event, function)),
                SyntaxRole::CognitiveMultiway => ("match", 1 + self.nesting(event, function)),
                SyntaxRole::CognitiveLabeledJump => ("labeled_jump", 1),
                _ => continue,
            };
            contributions.push(contribution(event, kind, value));
        }
        contributions.extend(self.logical_contributions(owned_events));
        contributions.sort_by_key(|item| (item.start_byte, item.end_byte, item.kind.clone()));
        let complexity = contributions
            .iter()
            .map(|item| item.value.max(0) as usize)
            .sum();
        (complexity, contributions)
    }
}

fn is_scored_event(role: SyntaxRole) -> bool {
    matches!(
        role,
        SyntaxRole::CognitiveIf
            | SyntaxRole::CognitiveElse
            | SyntaxRole::CognitiveLoop
            | SyntaxRole::CognitiveLetElse
            | SyntaxRole::CognitiveMultiway
            | SyntaxRole::CognitiveLogicalAnd
            | SyntaxRole::CognitiveLogicalOr
            | SyntaxRole::CognitiveLabeledJump
    )
}

fn contribution(event: &SyntaxEvent, kind: &str, value: usize) -> ComplexityContribution {
    ComplexityContribution {
        kind: kind.into(),
        value: value.min(i32::MAX as usize) as i32,
        start_byte: event.start_byte,
        end_byte: event.end_byte,
        line: event.start_row + 1,
    }
}

fn contains_range(outer: &std::ops::Range<usize>, inner: &std::ops::Range<usize>) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}
