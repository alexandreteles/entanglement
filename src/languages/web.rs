mod css;
mod exports;
mod facts;
mod html;
mod imports;
mod javascript;
mod module_facts;
mod reexports;
mod syntax;
mod typescript;

use tree_sitter::{Language, Tree};

use super::query::{QueryAnalyzer, QueryFacts};
use super::{
    CapturedTree, FileModuleLayout, FileModuleRules, LanguageHandler, LanguageSpec,
    ResolutionFamily,
};
use crate::Result;

const WEB_EXTENSIONS: &[&str] = &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs", "d.ts"];
const WEB_REMAPS: &[(&str, &[&str])] = &[
    ("js", &["ts", "tsx"]),
    ("jsx", &["tsx"]),
    ("mjs", &["mts"]),
    ("cjs", &["cts"]),
];
const WEB_INDEX_STEMS: &[&str] = &["index"];
const WEB_UNRESOLVED_PREFIXES: &[&str] = &["@/", "~/", "#"];
const WEB_MODULE_RULES: FileModuleRules = FileModuleRules {
    extensions: WEB_EXTENSIONS,
    remaps: WEB_REMAPS,
    index_stems: WEB_INDEX_STEMS,
    unresolved_prefixes: WEB_UNRESOLVED_PREFIXES,
    layout: FileModuleLayout::SlashRelative,
    module_bindings_shadow: false,
    wildcard_imports_shadow: false,
};

pub(super) const LANGUAGE_SPECS: &[LanguageSpec] = &[
    LanguageSpec {
        scope: "source.js",
        resolution_family: ResolutionFamily::FileModules,
        project_manifests: &["package.json", "tsconfig.json", "jsconfig.json"],
        file_module_rules: Some(&WEB_MODULE_RULES),
        build,
    },
    LanguageSpec {
        scope: "source.ts",
        resolution_family: ResolutionFamily::FileModules,
        project_manifests: &["package.json", "tsconfig.json", "jsconfig.json"],
        file_module_rules: Some(&WEB_MODULE_RULES),
        build,
    },
    LanguageSpec {
        scope: "source.tsx",
        resolution_family: ResolutionFamily::FileModules,
        project_manifests: &["package.json", "tsconfig.json", "jsconfig.json"],
        file_module_rules: Some(&WEB_MODULE_RULES),
        build,
    },
    LanguageSpec {
        scope: "source.html",
        resolution_family: ResolutionFamily::None,
        project_manifests: &[],
        file_module_rules: None,
        build,
    },
    LanguageSpec {
        scope: "source.css",
        resolution_family: ResolutionFamily::None,
        project_manifests: &[],
        file_module_rules: None,
        build,
    },
];

fn build(scope: &str, language: &Language) -> Result<Box<dyn LanguageHandler>> {
    match scope {
        "source.js" => javascript::build(language),
        "source.ts" | "source.tsx" => typescript::build(language),
        "source.html" => html::build(language),
        "source.css" => css::build(language),
        _ => Err(std::io::Error::other(format!("No web analyzer for scope {scope}")).into()),
    }
}

pub(super) struct Analyzer {
    query: QueryAnalyzer,
    captures: facts::Captures,
}

impl Analyzer {
    fn new(language: &Language, query_source: &str) -> Result<Self> {
        let query = QueryAnalyzer::new(language, query_source)?;
        let captures = facts::Captures::new(&query);
        Ok(Self { query, captures })
    }
}

impl LanguageHandler for Analyzer {
    fn capture(&self, tree: &Tree, source: &[u8], include_tokens: bool) -> Result<CapturedTree> {
        let mut facts = facts::CaptureFacts::new(tree.root_node().byte_range());
        let query = self
            .query
            .capture_with(tree, source, include_tokens, |_, captures, bytes| {
                facts.record(&self.captures, captures, bytes);
            });
        let semantic = facts.finish(tree.root_node());
        Ok(captured(query, semantic))
    }
}

pub(super) fn build_web(language: &Language, source: &str) -> Result<Box<dyn LanguageHandler>> {
    Ok(Box::new(Analyzer::new(language, source)?))
}

fn captured(query: QueryFacts, semantic: facts::SemanticFacts) -> CapturedTree {
    let mut captured = CapturedTree::from(query);
    captured.definitions = semantic.definitions;
    captured.imports = semantic.imports;
    captured.exports = semantic.exports;
    captured.references = semantic.references;
    captured.locals = semantic.locals;
    captured
}
