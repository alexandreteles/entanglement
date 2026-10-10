use std::collections::{HashMap, HashSet};

use super::super::{SyntaxEvent, SyntaxRole, cyclomatic::FunctionScope};
use super::Context;

impl<'a> Context<'a> {
    /// Build indexes for one syntax tree.
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

    pub(super) fn nesting(&self, event: &SyntaxEvent, function: &FunctionScope) -> usize {
        let suppressed = self.suppressed_controls(event);
        self.count_nesting(event, function, &suppressed)
    }

    fn suppressed_controls(&self, event: &SyntaxEvent) -> Vec<usize> {
        let mut suppressed = self
            .condition_parent
            .get(&event.node_id)
            .copied()
            .into_iter()
            .collect::<Vec<_>>();
        let mut current = self.parents.get(&event.node_id).copied().flatten();
        while let Some(node) = current {
            suppressed.extend(self.else_if_parent.get(&node).copied());
            suppressed.extend(self.condition_parent.get(&node).copied());
            current = self.parents.get(&node).copied().flatten();
        }
        suppressed
    }

    fn count_nesting(
        &self,
        event: &SyntaxEvent,
        function: &FunctionScope,
        suppressed: &[usize],
    ) -> usize {
        let mut result = 0;
        let mut current = self.parents.get(&event.node_id).copied().flatten();
        while let Some(node) = current {
            match self.roles.get(&node) {
                Some(SyntaxRole::CognitiveIf | SyntaxRole::CognitiveLetElse)
                    if !suppressed.contains(&node) =>
                {
                    result += 1;
                }
                Some(
                    SyntaxRole::CognitiveLoop
                    | SyntaxRole::CognitiveMultiway
                    | SyntaxRole::CognitiveClosure,
                ) => result += 1,
                Some(SyntaxRole::Function)
                    if self.function_ranges_by_node.get(&node) != Some(&function.range) =>
                {
                    result += 1;
                }
                _ => {}
            }
            current = self.parents.get(&node).copied().flatten();
        }
        result
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
