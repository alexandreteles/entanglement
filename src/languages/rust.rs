mod capture;
mod references;
mod scopes;

use capture::has_visibility;
use scopes::{lexical_scope, local_bindings, relative_module};

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use tree_sitter::{Query, QueryCursor, StreamingIterator, Tree};

use super::{CapturedTree, InjectionRequest, LanguageHandler};
use crate::Result;
use crate::metrics::{SyntaxEvent, SyntaxRole, cyclomatic::FunctionScope};
use crate::model::{Definition, DefinitionKind, ReferenceKind};

const ANALYSIS_QUERY: &str = include_str!("../queries/rust.scm");

pub(super) struct Analyzer {
    query: Query,
    captures: Captures,
}

struct Captures {
    roles: HashMap<u32, SyntaxRole>,
    definition_ids: HashSet<u32>,
    local_scopes: HashMap<u32, LocalScopeKind>,
    reference_kinds: HashMap<u32, ReferenceKind>,
    syntax_node: u32,
    metric_function: u32,
    import: u32,
    reference_path: u32,
    local_pattern: u32,
    injection_content: u32,
    injection_language: Option<u32>,
    name: u32,
    definitions: Vec<(u32, DefinitionKind)>,
    reference_call: u32,
    reference_implementation: u32,
    injection_patterns: HashMap<usize, InjectionProperties>,
}

#[derive(Clone, Default)]
struct InjectionProperties {
    language: Option<String>,
    language_capture: Option<u32>,
    include_children: bool,
    priority: i32,
}

#[derive(Default)]
struct NodeGraph<'a> {
    nodes: HashMap<usize, tree_sitter::Node<'a>>,
    source: &'a [u8],
}

impl NodeGraph<'_> {
    fn text(&self, id: usize) -> Option<String> {
        let node = *self.nodes.get(&id)?;
        (node.child_count() == 0).then(|| node_text(node, self.source))
    }
}

#[derive(Clone)]
struct RawDefinition {
    node_id: usize,
    range: Range<usize>,
    name_range: Range<usize>,
    name: String,
    kind: DefinitionKind,
    body: Option<Range<usize>>,
    inline_module: bool,
    external_module: bool,
}

struct RawImport {
    node_id: usize,
    path: Vec<String>,
    alias: Option<String>,
    range: Range<usize>,
    is_public: bool,
}

#[derive(Clone)]
struct RawReference {
    node_id: usize,
    path: Vec<String>,
    range: Range<usize>,
    kind: ReferenceKind,
}

struct RawPath {
    node_id: usize,
    kind: ReferenceKind,
}

struct RawLocal {
    owner: usize,
    pattern: Option<usize>,
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

#[derive(Clone)]
struct ModuleSpan {
    name: String,
    body: Range<usize>,
}

#[derive(Default)]
struct CaptureFacts<'a> {
    events: Vec<SyntaxEvent>,
    parents: HashMap<usize, Option<usize>>,
    graph: NodeGraph<'a>,
    functions: Vec<FunctionScope>,
    definitions: Vec<RawDefinition>,
    import_roots: Vec<usize>,
    references: Vec<RawReference>,
    paths: Vec<RawPath>,
    locals: Vec<RawLocal>,
    injections: Vec<InjectionRequest>,
}

impl LanguageHandler for Analyzer {
    fn capture(&self, tree: &Tree, source: &[u8], include_tokens: bool) -> Result<CapturedTree> {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&self.query, tree.root_node(), source);
        let mut facts = CaptureFacts {
            graph: NodeGraph {
                nodes: HashMap::new(),
                source,
            },
            ..Default::default()
        };

        while let Some(query_match) = matches.next() {
            let captures = query_match.captures();
            facts.record_syntax(&self.captures, captures, source);
            facts.record_tags(&self.captures, captures, source);
            facts.record_injection(&self.captures, query_match.pattern_index, captures, source);
        }
        facts.normalize(tree, include_tokens)
    }
}

impl<'a> CaptureFacts<'a> {
    fn normalize(mut self, tree: &Tree, include_tokens: bool) -> Result<CapturedTree> {
        deduplicate(&mut self.definitions, |item| {
            (item.range.start, item.range.end)
        });
        let mut modules: Vec<_> = self
            .definitions
            .iter()
            .filter(|item| item.inline_module)
            .filter_map(|item| {
                Some(ModuleSpan {
                    name: item.name.clone(),
                    body: item.body.clone()?,
                })
            })
            .collect();
        modules.sort_by_key(|module| (module.body.end - module.body.start, module.body.start));
        modules.reverse();
        let root_range = tree.root_node().start_byte()..tree.root_node().end_byte();
        let mut definitions = self.normalize_definitions(&modules, &root_range)?;
        let mut imports = self.normalize_imports(&modules, &root_range);
        let mut references = self.normalize_references(&modules, &root_range);
        let locals = self
            .locals
            .iter()
            .flat_map(|local| local_bindings(local, &self.graph))
            .collect();

        deduplicate(&mut self.events, |event| (event.role, event.node_id));
        deduplicate(&mut self.functions, |function| {
            (function.range.start, function.range.end)
        });
        deduplicate(&mut self.injections, |injection| {
            (
                injection.range.start,
                injection.range.end,
                injection.language.clone(),
            )
        });
        deduplicate(&mut references, |reference| {
            (reference.start_byte, reference.path.clone(), reference.kind)
        });
        deduplicate(&mut definitions, |definition| {
            (
                definition.start_byte,
                definition.end_byte,
                definition.kind.clone(),
            )
        });
        deduplicate(&mut imports, |import| {
            (
                import.path.clone(),
                import.alias.clone(),
                import.scope_start,
            )
        });

        let tokens = if include_tokens {
            super::rust_tokens::capture(tree.root_node(), self.graph.source)
        } else {
            Vec::new()
        };
        Ok(CapturedTree {
            events: self.events,
            parents: self.parents,
            functions: self.functions,
            tokens,
            definitions,
            imports,
            references,
            locals,
            injections: self.injections,
        })
    }

    fn normalize_definitions(
        &self,
        modules: &[ModuleSpan],
        root: &Range<usize>,
    ) -> Result<Vec<Definition>> {
        self.definitions
            .iter()
            .map(|item| {
                self.graph.nodes.get(&item.node_id)?;
                let module = relative_module(item.range.start, modules);
                let (scope_start, scope_end) = lexical_scope(item.node_id, true, &self.graph, root);
                Some(Definition {
                    name: item.name.clone(),
                    kind: item.kind.clone(),
                    module,
                    start_byte: item.range.start,
                    end_byte: item.range.end,
                    scope_start,
                    scope_end,
                    is_public: has_visibility(item.node_id, &self.graph),
                    inline_module: item.inline_module,
                    external_module: item.external_module,
                })
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| -> crate::Error {
                std::io::Error::other("A definition node is missing from the query capture graph")
                    .into()
            })
    }
}

fn node_text(node: tree_sitter::Node<'_>, source: &[u8]) -> String {
    String::from_utf8_lossy(&source[node.byte_range()]).into_owned()
}

fn deduplicate<T, K: Eq + std::hash::Hash>(values: &mut Vec<T>, key: impl Fn(&T) -> K) {
    let mut seen = HashSet::new();
    values.retain(|value| seen.insert(key(value)));
}
