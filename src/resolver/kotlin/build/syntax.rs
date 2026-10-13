use std::collections::HashSet;

use crate::graph::SyntaxGraph;

use super::lexer::Token;

#[derive(Default)]
pub(super) struct KotlinSyntax {
    valid: bool,
    identifiers: HashSet<(usize, usize)>,
    strings: HashSet<(usize, usize)>,
    control_ranges: Vec<(usize, usize)>,
}

impl KotlinSyntax {
    pub(super) fn parse(source: &str) -> Self {
        let mut parser = tree_sitter::Parser::new();
        let language: tree_sitter::Language = tree_sitter_kotlin::LANGUAGE.into();
        if parser.set_language(&language).is_err() {
            return Self::default();
        }
        let Some(tree) = parser.parse(source, None) else {
            return Self::default();
        };

        let root = tree.root_node();
        let graph = SyntaxGraph::from_tree(root, source.as_bytes());
        let mut evidence = Self {
            valid: !root.has_error(),
            ..Self::default()
        };
        let mut pending = vec![root.id()];
        while let Some(id) = pending.pop() {
            let Some(node) = graph.node(id) else {
                continue;
            };
            evidence.record(graph.kind(id), node.start_byte(), node.end_byte());
            pending.extend(graph.children(id).iter().copied());
        }
        evidence
    }

    fn record(&mut self, kind: Option<&str>, start: usize, end: usize) {
        let range = (start, end);
        match kind {
            Some("simple_identifier") => {
                self.identifiers.insert(range);
            }
            Some("string_literal") => {
                self.strings.insert(range);
            }
            Some(
                "if_expression" | "when_expression" | "for_statement" | "while_statement"
                | "do_while_statement" | "try_expression",
            ) => self.control_ranges.push(range),
            _ => {}
        }
    }

    pub(super) fn permits_identifier(&self, token: &Token) -> bool {
        !self.valid || self.identifiers.contains(&(token.start, token.end))
    }

    pub(super) fn permits_string(&self, token: &Token) -> bool {
        !self.valid || self.strings.contains(&(token.start, token.end))
    }

    pub(super) fn outside_control_flow(&self, token: &Token) -> bool {
        !self
            .control_ranges
            .iter()
            .any(|(start, end)| *start <= token.start && token.end <= *end)
    }
}
