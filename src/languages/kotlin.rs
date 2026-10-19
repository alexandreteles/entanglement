use crate::languages::syntax::ancestors;
use crate::languages::syntax::children as named_children;
mod conventions;
mod facts;
mod scopes;
mod syntax;
mod tokens;

pub(crate) use syntax::lexer::skip_comment;

use std::borrow::Cow;

use tree_sitter::{Language, Node, Tree};

use super::query::QueryAnalyzer;
use super::{CapturedTree, LanguageHandler, LanguageSpec, ResolutionFamily};
use crate::Result;
use crate::metrics::{SyntaxEvent, SyntaxRole};

const KOTLIN_PROJECT_MANIFESTS: &[&str] = &[
    "settings.gradle.kts",
    "build.gradle.kts",
    "settings.gradle",
    "build.gradle",
    "pom.xml",
];

pub(super) const LANGUAGE_SPEC: LanguageSpec = LanguageSpec {
    scope: "source.kotlin",
    resolution_family: ResolutionFamily::KotlinModules,
    project_manifests: KOTLIN_PROJECT_MANIFESTS,
    file_module_rules: None,
    build,
};

pub(super) fn build(_: &str, language: &Language) -> Result<Box<dyn LanguageHandler>> {
    let query = QueryAnalyzer::new(language, include_str!("../queries/kotlin.scm"))?;
    Ok(Box::new(Analyzer { query }))
}

struct Analyzer {
    query: QueryAnalyzer,
}

impl LanguageHandler for Analyzer {
    fn incremental_safe(&self) -> bool {
        false
    }

    fn parser_source<'a>(&self, source: &'a [u8]) -> Cow<'a, [u8]> {
        syntax::mask_context_parameters(source)
            .map(Cow::Owned)
            .unwrap_or(Cow::Borrowed(source))
    }

    fn capture(&self, tree: &Tree, source: &[u8], include_tokens: bool) -> Result<CapturedTree> {
        let root = tree.root_node();
        let included_ranges = tree.included_ranges();
        let contexts = if included_ranges.is_empty() {
            syntax::context_parameters_at(source, 0)
        } else {
            included_ranges
                .iter()
                .filter_map(|range| {
                    let start = range.start_byte.min(source.len());
                    let end = range.end_byte.min(source.len());
                    (start <= end).then(|| (start, &source[start..end]))
                })
                .flat_map(|(base, slice)| syntax::context_parameters_at(slice, base))
                .collect()
        };
        let scope_index = scopes::ScopeIndex::new(root, source, &contexts);
        let mut shared = self.query.capture(tree, source, false);
        syntax::remove_else_events_for_else_if(scope_index.nodes(), &mut shared.events);
        syntax::append_kotlin_decision_events(
            scope_index.nodes(),
            &mut shared.events,
            &mut shared.parents,
        );
        syntax::append_context_terminals(source, &contexts, &mut shared.events);
        let query_functions = std::mem::take(&mut shared.functions);
        shared.functions = scopes::capture(&scope_index, query_functions);
        shared.events.retain_mut(|event| {
            if event.role != SyntaxRole::Function {
                return true;
            }
            if let Some(function) = shared
                .functions
                .iter()
                .filter(|function| {
                    function.range.end == event.end_byte
                        && (function.range.start == event.start_byte
                            || contexts.iter().any(|context| {
                                context.range.start == function.range.start
                                    && context.range.end <= event.start_byte
                                    && context.declaration_start <= event.start_byte
                                    && event.start_byte <= context.declaration_keyword
                            }))
                })
                .max_by_key(|function| function.range.start)
            {
                event.start_byte = function.range.start;
                event.start_row = function.line - 1;
                true
            } else {
                false
            }
        });
        if let Some(script) = shared
            .functions
            .iter()
            .find(|function| function.name.starts_with("script@"))
        {
            shared.events.push(SyntaxEvent {
                role: SyntaxRole::Function,
                start_byte: script.range.start,
                end_byte: script.range.end,
                start_row: script.line - 1,
                end_row: root.end_position().row,
                end_column: root.end_position().column,
                node_id: root.id(),
                parent_id: None,
                terminal: false,
            });
            shared.parents.entry(root.id()).or_insert(None);
        }
        if include_tokens {
            shared.tokens = tokens::capture(root, source, &contexts);
        }

        let semantic = facts::capture(&scope_index);
        let mut captured = CapturedTree::from(shared).with_semantics(semantic);
        captured.semantic.resolution_family = Some(ResolutionFamily::KotlinModules);
        captured.semantic.file_module_rules = LANGUAGE_SPEC.file_module_rules;
        Ok(captured)
    }
}

fn all_nodes(root: Node<'_>) -> Vec<Node<'_>> {
    let mut nodes = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        nodes.push(node);
        pending.extend(
            (0..node.child_count())
                .rev()
                .filter_map(|index| node.child(index)),
        );
    }
    nodes.sort_by_key(|node| (node.start_byte(), node.end_byte()));
    nodes
}
