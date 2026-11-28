use crate::languages::{CapturedTree, csharp::conditional::Line};
use crate::metrics::{SyntaxEvent, SyntaxRole};
use std::collections::{HashMap, HashSet};
use tree_sitter::Tree;

#[derive(Default)]
pub(super) struct Physical {
    pub captured: CapturedTree,
    nodes: HashMap<(usize, usize, u16), usize>,
    events: HashSet<(usize, usize, SyntaxRole)>,
    tokens: HashSet<(usize, usize)>,
}

/// Merge physical syntax nodes before the common metric algorithms assign ownership.
impl Physical {
    pub fn add(&mut self, tree: &Tree, mut capture: CapturedTree) {
        let mut mapping = HashMap::new();
        let mut pending = vec![tree.root_node()];
        while let Some(node) = pending.pop() {
            let next = self.nodes.len();
            let id = *self
                .nodes
                .entry((node.start_byte(), node.end_byte(), node.kind_id()))
                .or_insert(next);
            mapping.insert(node.id(), id);
            pending.extend(node.children(&mut node.walk()));
        }
        for (old, parent) in capture.parents {
            if let Some(id) = mapping.get(&old) {
                self.captured
                    .parents
                    .entry(*id)
                    .or_insert(parent.and_then(|p| mapping.get(&p).copied()));
            }
        }
        for mut event in capture.events {
            if !self
                .events
                .insert((event.start_byte, event.end_byte, event.role))
            {
                continue;
            }
            event.node_id = mapping
                .get(&event.node_id)
                .copied()
                .unwrap_or(event.node_id);
            event.parent_id = event.parent_id.and_then(|id| mapping.get(&id).copied());
            self.captured.events.push(event);
        }
        self.captured.functions.append(&mut capture.functions);
        self.captured.tokens.extend(
            capture
                .tokens
                .into_iter()
                .filter(|t| self.tokens.insert((t.start_byte, t.end_byte))),
        );
    }

    pub fn directives(&mut self, lines: &[Line], raw: CapturedTree) {
        self.captured
            .tokens
            .extend(raw.tokens.into_iter().filter(|t| {
                let i = lines.partition_point(|line| line.range.end <= t.start_byte);
                lines.get(i).is_some_and(|line| {
                    line.range.start <= t.start_byte && t.end_byte <= line.range.end
                })
            }));
        self.captured
            .tokens
            .sort_by_key(|t| (t.start_byte, t.end_byte));
        for line in lines {
            self.captured.events.push(SyntaxEvent {
                role: SyntaxRole::Node,
                start_byte: line.range.start,
                end_byte: line.range.end,
                start_row: line.row,
                end_row: line.row,
                end_column: 1,
                node_id: usize::MAX - line.row,
                parent_id: None,
                terminal: true,
            });
        }
    }
}
