//! C# syntax and source evidence from the upstream grammar.
use super::query::QueryAnalyzer;
use super::{CapturedTree, LanguageHandler, LanguageSpec, ResolutionFamily};
use crate::Result;
use tree_sitter::{Language, Tree};

mod scopes;

pub(super) const LANGUAGE_SPEC: LanguageSpec = LanguageSpec {
    scope: "source.cs",
    resolution_family: ResolutionFamily::None,
    project_manifests: &[],
    file_module_rules: None,
    build,
};

struct Analyzer(QueryAnalyzer);

fn build(_: &str, language: &Language) -> Result<Box<dyn LanguageHandler>> {
    let source = format!(
        "{}\n{}",
        tree_sitter_c_sharp::TAGS_QUERY,
        include_str!("../queries/csharp.scm")
    );
    Ok(Box::new(Analyzer(QueryAnalyzer::new(language, &source)?)))
}

impl LanguageHandler for Analyzer {
    fn capture(&self, tree: &Tree, source: &[u8], include_tokens: bool) -> Result<CapturedTree> {
        let mut captured = CapturedTree::from(self.0.capture(tree, source, include_tokens));
        scopes::complete(tree.root_node(), source, &mut captured);
        Ok(captured)
    }
}
