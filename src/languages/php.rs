//! PHP metrics and static syntax facts from the upstream grammar.

mod bindings;
mod declarations;
mod facts;
mod references;
mod syntax;

use tree_sitter::{Language, Tree};

use super::query::QueryAnalyzer;
use super::{CapturedTree, LanguageHandler, LanguageSpec, ResolutionFamily};
use crate::Result;

pub(super) const LANGUAGE_SPEC: LanguageSpec = LanguageSpec {
    scope: "source.php",
    resolution_family: ResolutionFamily::PhpNamespaces,
    project_manifests: &["composer.json"],
    file_module_rules: None,
    build,
};

fn build(_: &str, language: &Language) -> Result<Box<dyn LanguageHandler>> {
    Ok(Box::new(Analyzer(QueryAnalyzer::new(
        language,
        include_str!("../queries/php.scm"),
    )?)))
}

struct Analyzer(QueryAnalyzer);

impl LanguageHandler for Analyzer {
    fn capture(&self, tree: &Tree, source: &[u8], include_tokens: bool) -> Result<CapturedTree> {
        let mut captured: CapturedTree = self.0.capture(tree, source, include_tokens).into();
        facts::capture(tree.root_node(), source, &mut captured);
        Ok(captured)
    }
}

/// Shared source and output state for one PHP syntax traversal.
struct Capture<'a> {
    source: &'a [u8],
    php: &'a mut crate::graph::PhpFacts,
    captured: &'a mut CapturedTree,
}
