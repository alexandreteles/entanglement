use std::collections::{BTreeMap, HashSet};

use tree_sitter::QueryCapture;

use crate::metrics::{SyntaxEvent, SyntaxRole, cyclomatic::FunctionScope};

use super::{QueryAnalyzer, QueryFacts, ranges};

impl QueryFacts {
    pub(super) fn record_syntax(
        &mut self,
        analyzer: &QueryAnalyzer,
        captures: &[QueryCapture<'_>],
        source: &[u8],
        included: &[tree_sitter::Range],
    ) {
        for capture in captures {
            let node = capture.node;
            if Some(capture.index) == analyzer.syntax_node {
                self.parents
                    .insert(node.id(), node.parent().map(|parent| parent.id()));
            }
            if let Some(role) = analyzer.roles.get(&capture.index) {
                match role {
                    SyntaxRole::Node if node.child_count() == 0 => {
                        for range in ranges::node_segments(node, included) {
                            self.events.push(event_range(*role, node, range));
                        }
                    }
                    SyntaxRole::Node | SyntaxRole::InjectionContent => {
                        self.events.push(event(*role, node));
                    }
                    SyntaxRole::Comment => {
                        for range in ranges::node_segments(node, included) {
                            self.events.push(event_range(*role, node, range));
                        }
                    }
                    _ if ranges::fully_included(node, included) => {
                        self.events.push(event(*role, node));
                    }
                    _ => {}
                }
            }
            if Some(capture.index) == analyzer.function {
                if !ranges::fully_included(node, included) {
                    continue;
                }
                let explicit_name = analyzer.function_name.and_then(|name_id| {
                    captures
                        .iter()
                        .filter(|item| item.index == name_id)
                        .min_by_key(|item| distance(item.node.start_byte(), node.start_byte()))
                        .map(|item| node_text(item.node, source))
                });
                let name = explicit_name
                    .or_else(|| {
                        node.child_by_field_name("name")
                            .map(|name| node_text(name, source))
                    })
                    .unwrap_or_else(|| format!("anonymous@{}", node.start_position().row + 1));
                self.functions.push(FunctionScope {
                    name,
                    range: node.start_byte()..node.end_byte(),
                    line: node.start_position().row + 1,
                });
            }
        }
    }

    pub(super) fn deduplicate(&mut self) {
        deduplicate(&mut self.events, |event| {
            (event.role, event.node_id, event.start_byte, event.end_byte)
        });
        deduplicate_functions(&mut self.functions);
    }
}

pub(super) fn event(role: SyntaxRole, node: tree_sitter::Node<'_>) -> SyntaxEvent {
    event_range(role, node, node.range())
}

fn event_range(
    role: SyntaxRole,
    node: tree_sitter::Node<'_>,
    range: tree_sitter::Range,
) -> SyntaxEvent {
    SyntaxEvent {
        role,
        start_byte: range.start_byte,
        end_byte: range.end_byte,
        start_row: range.start_point.row,
        end_row: range.end_point.row,
        end_column: range.end_point.column,
        node_id: node.id(),
        parent_id: node.parent().map(|parent| parent.id()),
        terminal: node.child_count() == 0,
    }
}

pub(super) fn node_text(node: tree_sitter::Node<'_>, source: &[u8]) -> String {
    String::from_utf8_lossy(&source[node.byte_range()]).into_owned()
}

fn distance(left: usize, right: usize) -> usize {
    left.abs_diff(right)
}

fn deduplicate<T, K: Eq + std::hash::Hash>(values: &mut Vec<T>, key: impl Fn(&T) -> K) {
    let mut seen = HashSet::new();
    values.retain(|item| seen.insert(key(item)));
}

fn deduplicate_functions(functions: &mut Vec<FunctionScope>) {
    let mut unique = BTreeMap::new();
    for function in functions.drain(..) {
        let key = (function.range.start, function.range.end);
        match unique.entry(key).or_insert_with(|| function.clone()) {
            current if is_anonymous(&current.name) && !is_anonymous(&function.name) => {
                *current = function;
            }
            _ => {}
        }
    }
    functions.extend(unique.into_values());
}

fn is_anonymous(name: &str) -> bool {
    name.starts_with("anonymous@")
}
