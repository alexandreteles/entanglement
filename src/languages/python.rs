mod facts;
mod syntax;

use tree_sitter::{Language, Node, Tree};

use super::query::QueryAnalyzer;
use super::{CapturedTree, LanguageHandler, LanguageSpec, ResolutionFamily};
use crate::Result;

const PYTHON_PROJECT_MANIFESTS: &[&str] = &["pyproject.toml", "setup.py", "setup.cfg"];

pub(super) const LANGUAGE_SPEC: LanguageSpec = LanguageSpec {
    scope: "source.python",
    resolution_family: ResolutionFamily::FileModules,
    project_manifests: PYTHON_PROJECT_MANIFESTS,
    file_module_rules: Some(&facts::PYTHON_FILE_MODULE_RULES),
    build,
};

pub(super) fn build(_: &str, language: &Language) -> Result<Box<dyn LanguageHandler>> {
    let query = include_str!("../queries/python.scm");
    let query = QueryAnalyzer::new(language, query)?;
    let content = query.capture_id("injection.content");
    Ok(Box::new(Analyzer { query, content }))
}

struct Analyzer {
    query: QueryAnalyzer,
    content: Option<u32>,
}

impl LanguageHandler for Analyzer {
    fn capture(&self, tree: &Tree, source: &[u8], include_tokens: bool) -> Result<CapturedTree> {
        let mut unanalyzed = Vec::new();
        let mut shared = self
            .query
            .capture_with(tree, source, include_tokens, |_, captures, _| {
                for capture in captures
                    .iter()
                    .filter(|item| Some(item.index) == self.content)
                {
                    if changes_runtime_text(capture.node, source) {
                        unanalyzed.push((capture.node.start_byte(), capture.node.end_byte()));
                    }
                }
            });
        syntax::normalize_comprehension_parents(tree.root_node(), &mut shared.parents);
        for injection in &mut shared.injections {
            if unanalyzed.contains(&(injection.range.start, injection.range.end)) {
                injection.guest_ranges.clear();
            }
        }

        let mut captured = CapturedTree::from(shared);
        let semantic = facts::capture(tree.root_node(), source);
        captured.definitions = semantic.definitions;
        captured.imports = semantic.imports;
        captured.exports = semantic.exports;
        captured.references = semantic.references;
        captured.locals = semantic.locals;
        Ok(captured)
    }
}

fn changes_runtime_text(string: Node<'_>, source: &[u8]) -> bool {
    let mut cursor = string.walk();
    let Some(start) = string
        .named_children(&mut cursor)
        .find(|child| child.kind() == "string_start")
    else {
        return true;
    };
    let (raw, bytes) = string_flags(start, source);
    if bytes {
        return true;
    }
    let mut cursor = string.walk();
    for child in string.named_children(&mut cursor) {
        match child.kind() {
            "string_content" if has_runtime_escape(child, raw) => return true,
            "interpolation" if has_format_transform(child) => return true,
            _ => {}
        }
    }
    false
}

fn string_flags(start: Node<'_>, source: &[u8]) -> (bool, bool) {
    let Some(text) = source.get(start.byte_range()) else {
        return (false, false);
    };
    let Some(quote) = text.iter().position(|byte| matches!(byte, b'\'' | b'"')) else {
        return (false, false);
    };
    let prefix = &text[..quote];
    (
        prefix.iter().any(|byte| matches!(byte, b'r' | b'R')),
        prefix.iter().any(|byte| matches!(byte, b'b' | b'B')),
    )
}

fn has_runtime_escape(content: Node<'_>, raw: bool) -> bool {
    let mut pending = vec![content];
    while let Some(node) = pending.pop() {
        if node.kind() == "escape_interpolation" || (!raw && node.kind() == "escape_sequence") {
            return true;
        }
        let mut cursor = node.walk();
        pending.extend(node.named_children(&mut cursor));
    }
    false
}

fn has_format_transform(interpolation: Node<'_>) -> bool {
    let mut cursor = interpolation.walk();
    interpolation
        .named_children(&mut cursor)
        .any(|child| matches!(child.kind(), "format_specifier" | "type_conversion"))
}
