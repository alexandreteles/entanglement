mod captures;
mod facts;
pub(crate) mod literals;
mod scopes;
mod syntax;

use tree_sitter::{Language, Tree};

use super::query::QueryAnalyzer;
use super::{CapturedTree, LanguageHandler, LanguageSpec, ResolutionFamily};
use crate::Result;

pub(super) const LANGUAGE_SPEC: LanguageSpec = LanguageSpec {
    scope: "source.go",
    resolution_family: ResolutionFamily::GoPackages,
    project_manifests: &["go.mod"],
    file_module_rules: None,
    build,
};

pub(super) fn build(_: &str, language: &Language) -> Result<Box<dyn LanguageHandler>> {
    let query = QueryAnalyzer::new(language, include_str!("../queries/go.scm"))?;
    let captures = captures::Captures::new(&query);
    Ok(Box::new(Analyzer { query, captures }))
}

struct Analyzer {
    query: QueryAnalyzer,
    captures: captures::Captures,
}

impl LanguageHandler for Analyzer {
    fn capture(&self, tree: &Tree, source: &[u8], include_tokens: bool) -> Result<CapturedTree> {
        let mut semantic = facts::CaptureFacts::new(tree.root_node().byte_range());
        let shared =
            self.query
                .capture_with(tree, source, include_tokens, |_, captures, source| {
                    semantic.record(&self.captures, captures, source);
                });
        let semantic = semantic.finish();
        Ok(CapturedTree {
            definitions: semantic.definitions,
            imports: semantic.imports,
            references: semantic.references,
            locals: semantic.locals,
            ..CapturedTree::from(shared)
        })
    }
}
