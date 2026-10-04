use std::collections::{HashMap, HashSet};

use crate::model::ComplexityContribution;

use super::{SyntaxEvent, SyntaxRole, cyclomatic::FunctionScope};

/// Whether a syntax role is needed to calculate cognitive complexity.
///
/// Function and closure events are included because they affect nesting even
/// when they do not contribute a point themselves.
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

/// Preindexed syntax context for one language tree.
///
/// Build this once per tree, then call [`Context::analyze_functions`] to avoid
/// rescanning every captured event and rebuilding ancestry indexes per
/// function.
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

impl<'a> Context<'a> {
    /// Build the indexes used by all function metric calculations in a tree.
    pub fn new(events: &'a [SyntaxEvent], parents: &'a HashMap<usize, Option<usize>>) -> Self {
        let mut roles = HashMap::new();
        let mut function_ranges_by_node = HashMap::new();
        let mut if_nodes = HashSet::new();
        let mut pre_nesting_controls = HashSet::new();
        let mut condition_boundaries = HashSet::new();
        let mut else_if_boundaries = HashSet::new();
        let mut logical_expressions = HashSet::new();
        let mut parentheses = HashSet::new();
        for event in events {
            if matches!(
                event.role,
                SyntaxRole::Function
                    | SyntaxRole::CognitiveIf
                    | SyntaxRole::CognitiveLoop
                    | SyntaxRole::CognitiveLetElse
                    | SyntaxRole::CognitiveMultiway
                    | SyntaxRole::CognitiveClosure
            ) {
                roles.insert(event.node_id, event.role);
            }
            match event.role {
                SyntaxRole::Function => {
                    function_ranges_by_node.insert(event.node_id, event.range());
                }
                SyntaxRole::CognitiveIf => {
                    if_nodes.insert(event.node_id);
                    pre_nesting_controls.insert(event.node_id);
                }
                SyntaxRole::CognitiveConditionBoundary => {
                    condition_boundaries.insert(event.node_id);
                }
                SyntaxRole::CognitiveLetElse => {
                    pre_nesting_controls.insert(event.node_id);
                }
                SyntaxRole::CognitiveElseIf => {
                    else_if_boundaries.insert(event.node_id);
                }
                SyntaxRole::CognitiveLogicalExpression => {
                    logical_expressions.insert(event.node_id);
                }
                SyntaxRole::CognitiveParentheses => {
                    parentheses.insert(event.node_id);
                }
                _ => {}
            }
        }
        let mut else_if_parent = HashMap::new();
        for boundary in &else_if_boundaries {
            if let Some(parent_if) = nearest_ancestor(*boundary, parents, &if_nodes) {
                else_if_parent.insert(*boundary, parent_if);
            }
        }
        let mut condition_parent = HashMap::new();
        for boundary in &condition_boundaries {
            if let Some(control) = nearest_ancestor(*boundary, parents, &pre_nesting_controls) {
                condition_parent.insert(*boundary, control);
            }
        }
        Self {
            events,
            parents,
            roles,
            function_ranges_by_node,
            else_if_nodes: else_if_boundaries,
            else_if_parent,
            condition_parent,
            logical_expressions,
            parentheses,
        }
    }

    /// Calculate metrics for all functions, returning results in input order.
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

    fn logical_contributions(&self, owned_events: &[&SyntaxEvent]) -> Vec<ComplexityContribution> {
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
            let mut previous = None;
            for event in group.iter().copied() {
                if previous != Some(event.role) {
                    result.push(contribution(event, "logical_operator", 1));
                }
                previous = Some(event.role);
            }
        }
        result
    }

    fn nesting(&self, event: &SyntaxEvent, function: &FunctionScope) -> usize {
        let mut suppressed_ifs = Vec::new();
        if let Some(control) = self.condition_parent.get(&event.node_id) {
            suppressed_ifs.push(*control);
        }
        let mut current = self.parents.get(&event.node_id).copied().flatten();
        while let Some(node) = current {
            if let Some(parent_if) = self.else_if_parent.get(&node) {
                suppressed_ifs.push(*parent_if);
            }
            if let Some(control) = self.condition_parent.get(&node) {
                suppressed_ifs.push(*control);
            }
            current = self.parents.get(&node).copied().flatten();
        }

        current = self.parents.get(&event.node_id).copied().flatten();
        let mut result = 0;
        while let Some(node) = current {
            match self.roles.get(&node) {
                Some(SyntaxRole::CognitiveIf | SyntaxRole::CognitiveLetElse)
                    if !suppressed_ifs.contains(&node) =>
                {
                    result += 1
                }
                Some(
                    SyntaxRole::CognitiveLoop
                    | SyntaxRole::CognitiveMultiway
                    | SyntaxRole::CognitiveClosure,
                ) => result += 1,
                Some(SyntaxRole::Function)
                    if self.function_ranges_by_node.get(&node) != Some(&function.range) =>
                {
                    result += 1
                }
                _ => {}
            }
            current = self.parents.get(&node).copied().flatten();
        }
        result
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

fn logical_root(
    expression: usize,
    logical_expressions: &HashSet<usize>,
    parentheses: &HashSet<usize>,
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

fn nearest_ancestor(
    node_id: usize,
    parents: &HashMap<usize, Option<usize>>,
    targets: &HashSet<usize>,
) -> Option<usize> {
    let mut current = parents.get(&node_id).copied().flatten();
    while let Some(node) = current {
        if targets.contains(&node) {
            return Some(node);
        }
        current = parents.get(&node).copied().flatten();
    }
    None
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
