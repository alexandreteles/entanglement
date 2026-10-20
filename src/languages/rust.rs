mod capture;
mod references;
mod scopes;

use capture::has_visibility;
use scopes::{lexical_scope, local_bindings, relative_module};

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use tree_sitter::{Language, Node, Tree};

use super::query::{QueryAnalyzer, QueryFacts};
use super::{CapturedTree, LanguageHandler, LanguageSpec, ResolutionFamily};
use crate::Result;
use crate::model::{Definition, DefinitionKind, ReferenceKind};

const ANALYSIS_QUERY: &str = include_str!("../queries/rust.scm");

pub(super) const LANGUAGE_SPEC: LanguageSpec = LanguageSpec {
    scope: "source.rust",
    resolution_family: ResolutionFamily::RustCrates,
    project_manifests: &["Cargo.toml"],
    file_module_rules: None,
    build,
};

fn build(_: &str, language: &Language) -> Result<Box<dyn LanguageHandler>> {
    Ok(Box::new(Analyzer::new(language)?))
}

pub(super) struct Analyzer {
    query: QueryAnalyzer,
    captures: Captures,
}

struct Captures {
    local_scopes: HashMap<u32, LocalScopeKind>,
    reference_kinds: HashMap<u32, ReferenceKind>,
    import: u32,
    reference_path: u32,
    local_pattern: u32,
    name: u32,
    definitions: Vec<(u32, DefinitionKind)>,
    reference_call: u32,
    reference_implementation: u32,
}

struct RawDefinition<'tree> {
    node: Node<'tree>,
    name: Node<'tree>,
    kind: DefinitionKind,
}

struct RawImport<'tree> {
    node: Node<'tree>,
    path: Vec<String>,
    alias: Option<String>,
    is_public: bool,
}

struct RawReference<'tree> {
    node: Node<'tree>,
    path: Vec<String>,
    kind: ReferenceKind,
}

struct RawPath<'tree> {
    node: Node<'tree>,
    kind: ReferenceKind,
}

struct RawLocal<'tree> {
    owner: Node<'tree>,
    names: Vec<(String, Range<usize>)>,
    scope: LocalScopeKind,
}

#[derive(Clone, Copy)]
enum LocalScopeKind {
    Block,
    Function,
    Loop,
    Condition,
    Match,
    Closure,
}

struct ModuleSpan {
    name: String,
    body: Range<usize>,
}

#[derive(Default)]
struct CaptureFacts<'a> {
    source: &'a [u8],
    definitions: Vec<RawDefinition<'a>>,
    import_roots: Vec<Node<'a>>,
    references: Vec<RawReference<'a>>,
    paths: Vec<RawPath<'a>>,
    locals: Vec<RawLocal<'a>>,
}

impl LanguageHandler for Analyzer {
    fn capture(&self, tree: &Tree, source: &[u8], include_tokens: bool) -> Result<CapturedTree> {
        let mut facts = CaptureFacts {
            source,
            ..Default::default()
        };
        let common = self
            .query
            .capture_with(tree, source, false, |_, captures, source| {
                facts.record_semantics(&self.captures, captures, source);
                facts.record_tags(&self.captures, captures, source);
            });
        Ok(facts.finish(tree, common, include_tokens))
    }
}

impl<'a> CaptureFacts<'a> {
    fn finish(mut self, tree: &Tree, mut common: QueryFacts, include_tokens: bool) -> CapturedTree {
        deduplicate(&mut self.definitions, |item| {
            (item.node.start_byte(), item.node.end_byte())
        });
        let mut modules: Vec<_> = self
            .definitions
            .iter()
            .filter(|item| item.node.kind() == "mod_item")
            .filter_map(|item| {
                Some(ModuleSpan {
                    name: node_text(item.name, self.source),
                    body: item.node.child_by_field_name("body")?.byte_range(),
                })
            })
            .collect();
        modules.sort_by_key(|module| (module.body.end - module.body.start, module.body.start));
        modules.reverse();
        let root_range = tree.root_node().start_byte()..tree.root_node().end_byte();
        let definitions = self.normalize_definitions(&modules, &root_range);
        let mut imports = self.normalize_imports(&modules, &root_range);
        let mut references = self.normalize_references(&modules, &root_range);
        let locals = self.locals.iter().flat_map(local_bindings).collect();

        deduplicate(&mut common.events, |event| (event.role, event.node_id));
        deduplicate(&mut references, |reference| {
            (reference.start_byte, reference.path.clone(), reference.kind)
        });
        deduplicate(&mut imports, |import| {
            (
                import.path.clone(),
                import.alias.clone(),
                import.scope_start,
            )
        });

        common.tokens = if include_tokens {
            super::rust_tokens::capture(tree.root_node(), self.source)
        } else {
            Vec::new()
        };
        CapturedTree::from(common).with_semantics(super::CapturedSemantics {
            definitions,
            imports,
            references,
            locals,
            ..Default::default()
        })
    }

    fn normalize_definitions(
        &self,
        modules: &[ModuleSpan],
        root: &Range<usize>,
    ) -> Vec<Definition> {
        self.definitions
            .iter()
            .map(|item| {
                let node = item.node;
                let (scope_start, scope_end) = lexical_scope(node, true, root);
                Definition {
                    declaration: None,
                    name: node_text(item.name, self.source),
                    kind: item.kind.clone(),
                    module: relative_module(node.start_byte(), modules),
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                    scope_start,
                    scope_end,
                    is_public: has_visibility(node),
                    inline_module: node.kind() == "mod_item"
                        && node.child_by_field_name("body").is_some(),
                    external_module: node.kind() == "mod_item"
                        && node.child_by_field_name("body").is_none(),
                    context_id: 0,
                }
            })
            .collect()
    }
}

fn node_text(node: tree_sitter::Node<'_>, source: &[u8]) -> String {
    String::from_utf8_lossy(&source[node.byte_range()]).into_owned()
}

fn deduplicate<T, K: Eq + std::hash::Hash>(values: &mut Vec<T>, key: impl Fn(&T) -> K) {
    let mut seen = HashSet::new();
    values.retain(|value| seen.insert(key(value)));
}
