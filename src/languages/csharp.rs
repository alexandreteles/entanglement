//! C# syntax and source evidence from the upstream grammar.
use super::query::QueryAnalyzer;
use super::{CapturedTree, LanguageHandler, LanguageSpec, ResolutionFamily};
use crate::Result;
use tree_sitter::{Language, Tree};

pub(crate) mod conditional;
pub(crate) mod facts;
pub(crate) mod project;
mod scopes;
pub(crate) mod script;

pub(crate) fn is_source(path: &std::path::Path) -> bool {
    ["cs", "csx"]
        .iter()
        .any(|suffix| project::extension(path, suffix))
}

pub(super) const LANGUAGE_SPEC: LanguageSpec = LanguageSpec {
    scope: "source.cs",
    resolution_family: ResolutionFamily::CSharp,
    project_manifests: &[],
    file_module_rules: None,
    build,
};

struct Analyzer(&'static QueryAnalyzer);

fn build(_: &str, language: &Language) -> Result<Box<dyn LanguageHandler>> {
    Ok(Box::new(Analyzer(query(language)?)))
}

fn query(language: &Language) -> Result<&'static QueryAnalyzer> {
    static QUERY: std::sync::OnceLock<std::result::Result<QueryAnalyzer, String>> =
        std::sync::OnceLock::new();
    QUERY
        .get_or_init(|| {
            let source = format!(
                "{}\n{}",
                tree_sitter_c_sharp::TAGS_QUERY,
                include_str!("../queries/csharp.scm")
            );
            QueryAnalyzer::new(language, &source).map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|message| std::io::Error::other(message.clone()).into())
}

impl LanguageHandler for Analyzer {
    fn capture(&self, tree: &Tree, source: &[u8], include_tokens: bool) -> Result<CapturedTree> {
        let mut captured = CapturedTree::from(self.0.capture(tree, source, include_tokens));
        scopes::complete(tree.root_node(), source, &mut captured);
        captured.facts = facts::capture(tree.root_node(), source, &captured.functions);
        let ranges = tree.included_ranges();
        let directives = conditional::lines(tree.root_node(), source)
            .into_iter()
            .filter(|line| {
                ranges
                    .iter()
                    .any(|r| r.start_byte <= line.range.start && line.range.end <= r.end_byte)
            })
            .collect::<Vec<_>>();
        script::capture(&directives, &mut captured, include_tokens);
        captured
            .facts
            .semantic
            .csharp
            .as_mut()
            .expect("C# facts")
            .directives = directives;
        Ok(captured)
    }
}
