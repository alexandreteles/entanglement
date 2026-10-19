mod inline;

pub(super) use inline::inline_templates;

use tree_sitter::{Language, Tree};

use crate::Result;
use crate::languages::{CapturedTree, LanguageHandler};
use crate::metrics::cyclomatic::FunctionScope;

use super::super::query::QueryAnalyzer;

pub(super) fn build(language: &Language) -> Result<Box<dyn LanguageHandler>> {
    let query = QueryAnalyzer::new(
        language,
        &format!(
            "{}\n{}",
            include_str!("../../queries/html.scm"),
            include_str!("../../queries/angular.scm")
        ),
    )?;
    Ok(Box::new(Analyzer { query }))
}

struct Analyzer {
    query: QueryAnalyzer,
}

impl LanguageHandler for Analyzer {
    fn capture(&self, tree: &Tree, source: &[u8], include_tokens: bool) -> Result<CapturedTree> {
        let mut captured: CapturedTree = self.query.capture(tree, source, include_tokens).into();
        link_branches(tree.root_node(), &mut captured);
        captured.functions.push(FunctionScope {
            name: "<template>".into(),
            range: tree.root_node().byte_range(),
            line: tree.root_node().start_position().row + 1,
        });
        captured.tokens.retain_mut(|token| {
            let Some(node) = tree
                .root_node()
                .descendant_for_byte_range(token.start_byte, token.end_byte)
            else {
                return true;
            };
            match node.kind() {
                "text" | "tag_name" | "attribute_name" | "raw_text" => false,
                "control_keyword"
                | "special_keyword"
                | "conditional_operator"
                | "coalescing_operator"
                | "unary_operator"
                | "ternary_operator"
                | "pipe_operator" => {
                    token.kind = crate::metrics::halstead::HalsteadTokenKind::Operator;
                    true
                }
                _ => true,
            }
        });
        Ok(captured)
    }
}

fn text<'a>(node: tree_sitter::Node<'_>, source: &'a [u8]) -> &'a str {
    std::str::from_utf8(&source[node.byte_range()]).unwrap_or_default()
}

fn children(node: tree_sitter::Node<'_>) -> impl Iterator<Item = tree_sitter::Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .collect::<Vec<_>>()
        .into_iter()
}

fn link_branches(root: tree_sitter::Node<'_>, captured: &mut CapturedTree) {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        let mut paired_if = None;
        for child in children(node) {
            match child.kind() {
                "if_statement" => paired_if = Some(child.id()),
                "else_if_statement" | "else_statement" => {
                    if let Some(parent) = paired_if {
                        captured.parents.insert(child.id(), Some(parent));
                    }
                }
                "comment" => {}
                _ => paired_if = None,
            }
            pending.push(child);
        }
    }
}
