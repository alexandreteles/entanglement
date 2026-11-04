mod declarators;
mod scopes;
mod walker;

use tree_sitter::{Language, Tree};

use super::query::QueryAnalyzer;
use super::{CapturedTree, LanguageHandler, LanguageSpec, ResolutionFamily};
use crate::Result;

/// Clang tooling files sit at the project root, unlike build files that nest.
const PROJECT_MANIFESTS: &[&str] = &["compile_commands.json", "compile_flags.txt"];

pub(super) const C_SPEC: LanguageSpec = LanguageSpec {
    scope: "source.c",
    resolution_family: ResolutionFamily::CLinkage,
    project_manifests: PROJECT_MANIFESTS,
    file_module_rules: None,
    build: build_c,
};

pub(super) const CPP_SPEC: LanguageSpec = LanguageSpec {
    scope: "source.cpp",
    resolution_family: ResolutionFamily::CLinkage,
    project_manifests: PROJECT_MANIFESTS,
    file_module_rules: None,
    build: build_cpp,
};

fn build_c(_: &str, language: &Language) -> Result<Box<dyn LanguageHandler>> {
    let query = QueryAnalyzer::new(language, include_str!("../queries/c.scm"))?;
    Ok(Box::new(Analyzer { query, cpp: false }))
}

fn build_cpp(_: &str, language: &Language) -> Result<Box<dyn LanguageHandler>> {
    let source = concat!(
        include_str!("../queries/c.scm"),
        include_str!("../queries/cpp.scm")
    );
    let query = QueryAnalyzer::new(language, source)?;
    Ok(Box::new(Analyzer { query, cpp: true }))
}

struct Analyzer {
    query: QueryAnalyzer,
    /// C++ classes own their members; C structs only describe field layout.
    cpp: bool,
}

impl LanguageHandler for Analyzer {
    fn capture(&self, tree: &Tree, source: &[u8], include_tokens: bool) -> Result<CapturedTree> {
        let shared = self.query.capture(tree, source, include_tokens);
        let semantic = walker::facts(tree.root_node(), source, self.cpp);
        Ok(CapturedTree::from(shared).with_semantics(semantic))
    }
}
