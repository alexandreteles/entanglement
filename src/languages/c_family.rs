mod build;
pub(crate) mod conditional;
pub(crate) mod declarator;
mod directives;
mod flags;
mod lex;
pub(crate) mod physical;
pub(crate) mod project;
pub(crate) mod semantic;

use super::query::QueryAnalyzer;
use super::syntax::{named_children, text};
use super::{CapturedTree, LanguageHandler, LanguageSpec, ResolutionFamily};
use crate::Result;
use crate::metrics::SyntaxRole;
use std::collections::{HashMap, HashSet};
use tree_sitter::{Language, Tree};

pub(super) const SPECS: [LanguageSpec; 2] = [spec("source.c"), spec("source.cpp")];

const fn spec(scope: &'static str) -> LanguageSpec {
    LanguageSpec {
        scope,
        resolution_family: ResolutionFamily::CFamily,
        project_manifests: &["compile_commands.json", "CMakeLists.txt", "meson.build"],
        file_module_rules: None,
        build,
    }
}

struct Analyzer {
    query: QueryAnalyzer,
    cpp: bool,
}

fn build(scope: &str, language: &Language) -> Result<Box<dyn LanguageHandler>> {
    let cpp = scope == "source.cpp";
    let common = include_str!("../queries/c.scm");
    let query = if cpp {
        QueryAnalyzer::new(
            language,
            &format!("{common}\n{}", include_str!("../queries/cpp.scm")),
        )?
    } else {
        QueryAnalyzer::new(language, common)?
    };
    Ok(Box::new(Analyzer { query, cpp }))
}

impl LanguageHandler for Analyzer {
    fn capture(&self, tree: &Tree, source: &[u8], include_tokens: bool) -> Result<CapturedTree> {
        let function = self.query.capture_id("metric.function");
        let alternative = self.query.capture_id("metric.cognitive.else");
        let excluded = self.query.capture_id("c.unevaluated");
        let mut names = HashMap::new();
        let mut chained = HashSet::new();
        let mut unevaluated = Vec::new();
        let mut captured = self
            .query
            .capture_with(tree, source, false, |_, captures, source| {
                for capture in captures {
                    let node = capture.node;
                    let id = Some(capture.index);
                    if id == function {
                        let name = if node.kind() == "lambda_expression" {
                            node.parent()
                                .filter(|parent| parent.kind() == "init_declarator")
                                .and_then(|parent| parent.child_by_field_name("declarator"))
                                .map(|name| text(name, source))
                        } else {
                            node.child_by_field_name("declarator")
                                .and_then(|declarator| declarator::callable(declarator, source))
                                .map(|callable| callable.display(self.cpp))
                        };
                        if let Some(name) = name {
                            names.insert(node.start_byte(), name);
                        }
                    } else if id == alternative
                        && named_children(node).any(|child| child.kind() == "if_statement")
                    {
                        chained.insert(node.id());
                    } else if id == excluded {
                        unevaluated.push(node.byte_range());
                    }
                }
            });
        for scope in &mut captured.functions {
            if let Some(name) = names.remove(&scope.range.start) {
                scope.name = name;
            }
        }
        captured.events.retain(|event| {
            if event.role == SyntaxRole::CognitiveElse {
                return !chained.contains(&event.node_id);
            }
            matches!(event.role, SyntaxRole::Node | SyntaxRole::Comment)
                || !unevaluated
                    .iter()
                    .any(|range| range.start <= event.start_byte && event.end_byte <= range.end)
        });
        let mut captured =
            CapturedTree::from(captured).with_semantics(semantic::capture(tree, source));
        let ranges = tree
            .included_ranges()
            .into_iter()
            .map(|r| r.start_byte.min(source.len())..r.end_byte.min(source.len()));
        physical::capture(source, ranges, self.cpp, include_tokens, &mut captured);
        Ok(captured)
    }
}
