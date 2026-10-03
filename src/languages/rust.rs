use std::collections::{HashMap, HashSet};
use std::ops::Range;

use tree_sitter::{Language, Query, QueryCursor, StreamingIterator, Tree};

use super::{CapturedTree, InjectionRequest, LanguageHandler};
use crate::Result;
use crate::metrics::{SyntaxEvent, SyntaxRole, cyclomatic::FunctionScope};
use crate::model::{
    Definition, DefinitionKind, Import, LocalBinding, ModulePath, Reference, ReferenceKind,
};

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

impl Analyzer {
    pub(super) fn new(language: &Language) -> Result<Self> {
        let source = format!(
            "{}\n{}\n{}",
            tree_sitter_rust::TAGS_QUERY,
            tree_sitter_rust::INJECTIONS_QUERY,
            ANALYSIS_QUERY
        );
        let query = Query::new(language, &source)?;
        let id = |name| {
            query
                .capture_index_for_name(name)
                .expect("required capture")
        };
        let roles = [
            ("syntax.node", SyntaxRole::Node),
            ("comment", SyntaxRole::Comment),
            ("metric.function", SyntaxRole::Function),
            ("metric.condition", SyntaxRole::Condition),
            ("metric.logical_condition", SyntaxRole::LogicalCondition),
            ("metric.multiway", SyntaxRole::Multiway),
            ("metric.case", SyntaxRole::Case),
            ("import", SyntaxRole::Import),
            ("reference.path", SyntaxRole::Reference),
            ("reference.call", SyntaxRole::Reference),
            ("reference.implementation", SyntaxRole::Reference),
            ("reference.value", SyntaxRole::Reference),
            ("reference.type", SyntaxRole::Reference),
            ("injection.content", SyntaxRole::InjectionContent),
        ]
        .into_iter()
        .map(|(name, role)| (id(name), role))
        .collect();
        let definitions = [
            ("definition.function", DefinitionKind::Function),
            ("definition.method", DefinitionKind::Method),
            ("definition.class", DefinitionKind::Other),
            ("definition.interface", DefinitionKind::Trait),
            ("definition.module", DefinitionKind::Module),
            ("definition.macro", DefinitionKind::Macro),
            ("definition.constant", DefinitionKind::Constant),
            ("definition.static", DefinitionKind::Static),
        ]
        .into_iter()
        .map(|(name, kind)| (id(name), kind))
        .collect::<Vec<_>>();
        let definition_ids = definitions.iter().map(|(capture, _)| *capture).collect();
        let local_scopes = [
            ("local.declaration", LocalScopeKind::Block),
            ("local.parameter", LocalScopeKind::Function),
            ("local.loop", LocalScopeKind::Loop),
            ("local.condition", LocalScopeKind::Condition),
            ("local.match", LocalScopeKind::Match),
            ("local.closure", LocalScopeKind::Closure),
        ]
        .into_iter()
        .map(|(name, scope)| (id(name), scope))
        .collect();
        let reference_kinds = [
            ("reference.value", ReferenceKind::Value),
            ("reference.type", ReferenceKind::Type),
        ]
        .into_iter()
        .map(|(name, kind)| (id(name), kind))
        .collect();
        let captures = Captures {
            roles,
            definition_ids,
            local_scopes,
            reference_kinds,
            syntax_node: id("syntax.node"),
            metric_function: id("metric.function"),
            import: id("import"),
            reference_path: id("reference.path"),
            local_pattern: id("local.pattern"),
            injection_content: id("injection.content"),
            injection_language: query.capture_index_for_name("injection.language"),
            name: id("name"),
            definitions,
            reference_call: id("reference.call"),
            reference_implementation: id("reference.implementation"),
            injection_patterns: (0..query.pattern_count())
                .filter_map(|index| {
                    let mut properties = InjectionProperties::default();
                    let mut found = false;
                    for property in query.property_settings(index) {
                        match property.key.as_ref() {
                            "injection.language" => {
                                found = true;
                                properties.language = property.value.as_deref().map(str::to_owned);
                                properties.language_capture =
                                    property.capture_id.map(|id| id as u32);
                            }
                            "injection.include-children" => {
                                found = true;
                                properties.include_children =
                                    property.value.as_deref() != Some("false");
                            }
                            "injection.priority" => {
                                found = true;
                                properties.priority = property
                                    .value
                                    .as_deref()
                                    .and_then(|value| value.parse().ok())
                                    .unwrap_or_default();
                            }
                            _ => {}
                        }
                    }
                    found.then_some((index, properties))
                })
                .collect(),
        };
        Ok(Self { query, captures })
    }
}

impl LanguageHandler for Analyzer {
    fn capture(&self, tree: &Tree, source: &[u8]) -> Result<CapturedTree> {
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
        facts.normalize(tree)
    }
}

impl<'a> CaptureFacts<'a> {
    fn record_syntax(
        &mut self,
        query: &Captures,
        captures: &[tree_sitter::QueryCapture<'a>],
        source: &[u8],
    ) {
        for capture in captures {
            let node = capture.node;
            let parent = node.parent().map(|parent| parent.id());
            if capture.index == query.syntax_node {
                self.parents.insert(node.id(), parent);
                self.graph.nodes.insert(node.id(), node);
            }
            if let Some(role) = query.roles.get(&capture.index) {
                self.events.push(event(*role, node));
            }
            if query.definition_ids.contains(&capture.index) {
                self.events.push(event(SyntaxRole::Definition, node));
            }
            if capture.index == query.metric_function
                && let Some(name) = node.child_by_field_name("name")
            {
                self.functions.push(FunctionScope {
                    name: node_text(name, source),
                    range: node.start_byte()..node.end_byte(),
                    line: node.start_position().row + 1,
                });
            }
            if capture.index == query.reference_path {
                self.paths.push(RawPath {
                    node_id: node.id(),
                    kind: if node.kind() == "scoped_type_identifier" {
                        ReferenceKind::Type
                    } else {
                        ReferenceKind::Qualified
                    },
                });
            }
            if let Some(kind) = query.reference_kinds.get(&capture.index) {
                self.references.push(raw_reference(node, *kind, source));
            }
            if capture.index == query.import {
                self.import_roots.push(node.id());
            }
            if let Some(scope) = query.local_scopes.get(&capture.index) {
                self.locals.push(raw_local(captures, node, query, *scope));
            }
        }
    }

    fn record_tags(
        &mut self,
        query: &Captures,
        captures: &[tree_sitter::QueryCapture<'_>],
        source: &[u8],
    ) {
        for name in captures.iter().filter(|item| item.index == query.name) {
            let name_node = name.node;
            for (definition_id, kind) in &query.definitions {
                if let Some(definition) = captures.iter().find(|item| {
                    item.index == *definition_id
                        && item.node.start_byte() <= name_node.start_byte()
                        && name_node.end_byte() <= item.node.end_byte()
                }) {
                    self.definitions
                        .push(raw_definition(definition.node, name_node, kind, source));
                }
            }
            for (capture_id, kind) in [
                (query.reference_call, None),
                (query.reference_implementation, Some(ReferenceKind::Type)),
            ] {
                if captures.iter().any(|item| item.index == capture_id) {
                    let kind = kind.unwrap_or(if name_node.kind() == "field_identifier" {
                        ReferenceKind::Method
                    } else {
                        ReferenceKind::Call
                    });
                    self.references.push(raw_reference(name_node, kind, source));
                }
            }
        }
    }

    fn record_injection(
        &mut self,
        query: &Captures,
        pattern_index: usize,
        captures: &[tree_sitter::QueryCapture<'_>],
        source: &[u8],
    ) {
        let Some(content) = captures
            .iter()
            .find(|item| item.index == query.injection_content)
        else {
            return;
        };
        let has_language_capture = query
            .injection_language
            .is_some_and(|id| captures.iter().any(|capture| capture.index == id));
        if has_language_capture || query.injection_patterns.contains_key(&pattern_index) {
            self.events
                .push(event(SyntaxRole::InjectionLanguage, content.node));
        }
        let mut properties = query
            .injection_patterns
            .get(&pattern_index)
            .cloned()
            .unwrap_or_default();
        if properties.language.is_none() && properties.language_capture.is_none() {
            properties.language_capture = query.injection_language.filter(|_| has_language_capture);
        }
        if let Some(injection) = injection_from_match(&properties, captures, content.node, source) {
            self.injections.push(injection);
        }
    }

    fn normalize(mut self, tree: &Tree) -> Result<CapturedTree> {
        deduplicate(&mut self.definitions, |item| {
            (item.range.start, item.range.end)
        });
        let modules: Vec<_> = self
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

        Ok(CapturedTree {
            events: self.events,
            parents: self.parents,
            functions: self.functions,
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

    fn use_ranges(&self) -> Vec<Range<usize>> {
        self.import_roots
            .iter()
            .filter_map(|id| self.graph.nodes.get(id).map(tree_sitter::Node::byte_range))
            .collect()
    }

    fn binding_ranges(&self) -> Vec<Range<usize>> {
        self.locals
            .iter()
            .flat_map(|local| {
                local.pattern.map_or_else(
                    || {
                        self.graph
                            .nodes
                            .get(&local.owner)
                            .map(|node| vec![node.byte_range()])
                            .unwrap_or_default()
                    },
                    |pattern| {
                        binding_names(pattern, &self.graph)
                            .into_iter()
                            .map(|(_, range)| range)
                            .collect()
                    },
                )
            })
            .collect()
    }

    fn normalize_imports(&self, modules: &[ModuleSpan], root: &Range<usize>) -> Vec<Import> {
        let mut raw = Vec::new();
        for id in &self.import_roots {
            flatten_import(*id, has_visibility(*id, &self.graph), &self.graph, &mut raw);
        }
        raw.into_iter()
            .map(|import| {
                let module = relative_module(import.range.start, modules);
                let (scope_start, scope_end) =
                    lexical_scope(import.node_id, false, &self.graph, root);
                Import {
                    path: import.path,
                    alias: import.alias,
                    start_byte: import.range.start,
                    end_byte: import.range.end,
                    module,
                    scope_start,
                    scope_end,
                    is_public: import.is_public,
                }
            })
            .collect()
    }

    fn normalize_references(&self, modules: &[ModuleSpan], root: &Range<usize>) -> Vec<Reference> {
        let use_ranges = self.use_ranges();
        let binding_ranges = self.binding_ranges();
        let definition_ranges = self
            .definitions
            .iter()
            .map(|definition| definition.name_range.clone())
            .collect::<Vec<_>>();
        let call_ranges = self
            .references
            .iter()
            .filter(|reference| {
                matches!(reference.kind, ReferenceKind::Call | ReferenceKind::Method)
            })
            .map(|reference| reference.range.clone())
            .collect::<Vec<_>>();
        let mut references = self.references.clone();
        references.extend(self.qualified_paths(&use_ranges));
        let qualified_ranges = references
            .iter()
            .filter(|reference| {
                matches!(
                    reference.kind,
                    ReferenceKind::Qualified | ReferenceKind::Type
                )
            })
            .map(|reference| reference.range.clone())
            .collect::<Vec<_>>();
        references.retain(|reference| {
            !matches!(reference.kind, ReferenceKind::Value | ReferenceKind::Type)
                || !definition_ranges
                    .iter()
                    .chain(&use_ranges)
                    .chain(&binding_ranges)
                    .chain(&call_ranges)
                    .any(|range| contains(range, &reference.range))
                    && !qualified_ranges
                        .iter()
                        .any(|range| range != &reference.range && contains(range, &reference.range))
        });
        references
            .into_iter()
            .map(|reference| {
                let module = relative_module(reference.range.start, modules);
                let (scope_start, scope_end) =
                    lexical_scope(reference.node_id, false, &self.graph, root);
                Reference {
                    path: reference.path,
                    module,
                    kind: reference.kind,
                    start_byte: reference.range.start,
                    end_byte: reference.range.end,
                    scope_start,
                    scope_end,
                }
            })
            .collect()
    }

    fn qualified_paths(&self, use_ranges: &[Range<usize>]) -> Vec<RawReference> {
        let candidates = self
            .paths
            .iter()
            .filter_map(|path| {
                let node = self.graph.nodes.get(&path.node_id)?;
                let segments = path_segments(path.node_id, &self.graph);
                (!segments.is_empty()).then_some(RawReference {
                    node_id: path.node_id,
                    path: segments,
                    range: node.byte_range(),
                    kind: path.kind,
                })
            })
            .filter(|reference| {
                !use_ranges
                    .iter()
                    .any(|range| contains(range, &reference.range))
            })
            .collect::<Vec<_>>();
        let ranges = candidates
            .iter()
            .map(|reference| reference.range.clone())
            .collect::<Vec<_>>();
        candidates
            .into_iter()
            .filter(|inner| {
                !ranges
                    .iter()
                    .any(|outer| outer != &inner.range && contains(outer, &inner.range))
            })
            .collect()
    }
}

fn event(role: SyntaxRole, node: tree_sitter::Node<'_>) -> SyntaxEvent {
    SyntaxEvent {
        role,
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        start_row: node.start_position().row,
        end_row: node.end_position().row,
        end_column: node.end_position().column,
        node_id: node.id(),
        parent_id: node.parent().map(|parent| parent.id()),
        terminal: node.child_count() == 0,
    }
}

fn node_text(node: tree_sitter::Node<'_>, source: &[u8]) -> String {
    String::from_utf8_lossy(&source[node.byte_range()]).into_owned()
}

fn raw_reference(node: tree_sitter::Node<'_>, kind: ReferenceKind, source: &[u8]) -> RawReference {
    RawReference {
        node_id: node.id(),
        path: vec![node_text(node, source)],
        range: node.start_byte()..node.end_byte(),
        kind,
    }
}

fn raw_definition(
    node: tree_sitter::Node<'_>,
    name: tree_sitter::Node<'_>,
    kind: &DefinitionKind,
    source: &[u8],
) -> RawDefinition {
    RawDefinition {
        node_id: node.id(),
        range: node.start_byte()..node.end_byte(),
        name_range: name.start_byte()..name.end_byte(),
        name: node_text(name, source),
        kind: if is_method(node) {
            DefinitionKind::Method
        } else if *kind == DefinitionKind::Method {
            DefinitionKind::Function
        } else {
            definition_kind(kind, node.kind())
        },
        body: node
            .child_by_field_name("body")
            .map(|body| body.start_byte()..body.end_byte()),
        inline_module: node.kind() == "mod_item" && node.child_by_field_name("body").is_some(),
        external_module: node.kind() == "mod_item" && node.child_by_field_name("body").is_none(),
    }
}

fn raw_local(
    captures: &[tree_sitter::QueryCapture<'_>],
    node: tree_sitter::Node<'_>,
    query: &Captures,
    scope: LocalScopeKind,
) -> RawLocal {
    RawLocal {
        owner: node.id(),
        pattern: capture_node_id(captures, query.local_pattern),
        scope,
    }
}

fn definition_kind(kind: &DefinitionKind, node_kind: &str) -> DefinitionKind {
    if *kind != DefinitionKind::Other {
        return kind.clone();
    }
    match node_kind {
        "struct_item" => DefinitionKind::Struct,
        "enum_item" => DefinitionKind::Enum,
        "union_item" => DefinitionKind::Union,
        "type_item" => DefinitionKind::TypeAlias,
        _ => DefinitionKind::Other,
    }
}

fn is_method(node: tree_sitter::Node<'_>) -> bool {
    node.parent().is_some_and(|declarations| {
        declarations.kind() == "declaration_list"
            && declarations
                .parent()
                .is_some_and(|owner| matches!(owner.kind(), "impl_item" | "trait_item"))
    })
}

fn has_visibility(node_id: usize, graph: &NodeGraph<'_>) -> bool {
    let Some(node) = graph.nodes.get(&node_id) else {
        return false;
    };
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| child.kind() == "visibility_modifier")
}

fn capture_node_id(captures: &[tree_sitter::QueryCapture<'_>], id: u32) -> Option<usize> {
    captures
        .iter()
        .find(|capture| capture.index == id)
        .map(|capture| capture.node.id())
}

fn injection_from_match(
    properties: &InjectionProperties,
    captures: &[tree_sitter::QueryCapture<'_>],
    content: tree_sitter::Node<'_>,
    source: &[u8],
) -> Option<InjectionRequest> {
    let language = properties.language.clone().or_else(|| {
        properties.language_capture.and_then(|id| {
            captures
                .iter()
                .find(|capture| capture.index == id)
                .map(|capture| node_text(capture.node, source))
        })
    })?;
    let child_ranges = if properties.include_children {
        Vec::new()
    } else {
        let mut cursor = content.walk();
        content
            .named_children(&mut cursor)
            .map(|child| child.range())
            .collect()
    };
    Some(InjectionRequest {
        language,
        range: content.start_byte()..content.end_byte(),
        child_ranges,
        start_point: content.start_position(),
        end_point: content.end_position(),
        include_children: properties.include_children,
        priority: properties.priority,
    })
}

fn flatten_import(
    root: usize,
    is_public: bool,
    graph: &NodeGraph<'_>,
    imports: &mut Vec<RawImport>,
) {
    let Some(argument) = child_field(root, "argument", graph) else {
        return;
    };
    flatten_use_node(argument, &[], is_public, graph, imports);
}

fn flatten_use_node(
    node_id: usize,
    prefix: &[String],
    is_public: bool,
    graph: &NodeGraph<'_>,
    imports: &mut Vec<RawImport>,
) {
    let Some(node) = graph.nodes.get(&node_id).copied() else {
        return;
    };
    match node.kind() {
        "scoped_use_list" => {
            let mut path = prefix.to_vec();
            if let Some(path_node) = child_field(node_id, "path", graph) {
                path.extend(path_segments(path_node, graph));
            }
            if let Some(list) = child_field(node_id, "list", graph) {
                flatten_use_node(list, &path, is_public, graph, imports);
            }
        }
        "use_list" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if is_use_node(child.id(), graph) {
                    flatten_use_node(child.id(), prefix, is_public, graph, imports);
                }
            }
        }
        "use_as_clause" => {
            let mut path = prefix.to_vec();
            if let Some(path_node) = child_field(node_id, "path", graph) {
                path.extend(path_segments(path_node, graph));
            }
            let alias = child_field(node_id, "alias", graph).and_then(|alias| graph.text(alias));
            push_import(node_id, node, path, alias, is_public, imports);
        }
        "use_wildcard" => {
            let mut path = prefix.to_vec();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if is_path_node(child.id(), graph) {
                    path.extend(path_segments(child.id(), graph));
                }
            }
            path.push("*".into());
            push_import(node_id, node, path, None, is_public, imports);
        }
        "self" if !prefix.is_empty() => {
            push_import(node_id, node, prefix.to_vec(), None, is_public, imports)
        }
        _ => {
            let mut path = prefix.to_vec();
            path.extend(path_segments(node_id, graph));
            push_import(node_id, node, path, None, is_public, imports);
        }
    }
}

fn push_import(
    node_id: usize,
    node: tree_sitter::Node<'_>,
    path: Vec<String>,
    alias: Option<String>,
    is_public: bool,
    imports: &mut Vec<RawImport>,
) {
    if path.is_empty() {
        return;
    }
    imports.push(RawImport {
        node_id,
        path,
        alias,
        range: node.byte_range(),
        is_public,
    });
}

fn is_use_node(id: usize, graph: &NodeGraph<'_>) -> bool {
    graph.nodes.get(&id).is_some_and(|node| {
        matches!(
            node.kind(),
            "scoped_use_list"
                | "use_as_clause"
                | "use_list"
                | "use_wildcard"
                | "scoped_identifier"
                | "identifier"
                | "crate"
                | "self"
                | "super"
        )
    })
}

fn is_path_node(id: usize, graph: &NodeGraph<'_>) -> bool {
    graph.nodes.get(&id).is_some_and(|node| {
        matches!(
            node.kind(),
            "scoped_identifier"
                | "scoped_type_identifier"
                | "identifier"
                | "type_identifier"
                | "crate"
                | "self"
                | "super"
        )
    })
}

fn child_field(node_id: usize, field: &str, graph: &NodeGraph<'_>) -> Option<usize> {
    graph
        .nodes
        .get(&node_id)?
        .child_by_field_name(field)
        .map(|node| node.id())
}

fn path_segments(node_id: usize, graph: &NodeGraph<'_>) -> Vec<String> {
    let Some(node) = graph.nodes.get(&node_id).copied() else {
        return Vec::new();
    };
    if let (Some(path), Some(name)) = (
        child_field(node_id, "path", graph),
        child_field(node_id, "name", graph),
    ) {
        let mut segments = path_segments(path, graph);
        if let Some(name) = graph.text(name) {
            segments.push(name);
        }
        return segments;
    }
    if is_path_node(node_id, graph) {
        return graph.text(node_id).into_iter().collect();
    }
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| is_path_node(child.id(), graph))
        .flat_map(|child| path_segments(child.id(), graph))
        .collect()
}

fn local_bindings(local: &RawLocal, graph: &NodeGraph<'_>) -> Vec<LocalBinding> {
    let names = local.pattern.map_or_else(
        || {
            graph
                .nodes
                .get(&local.owner)
                .map(|node| vec![("self".to_owned(), node.byte_range())])
                .unwrap_or_default()
        },
        |pattern| binding_names(pattern, graph),
    );
    let scope = match local.scope {
        LocalScopeKind::Function => nearest_kind(local.owner, "function_item", graph)
            .and_then(|id| child_field(id, "body", graph)),
        LocalScopeKind::Block => {
            let Some(declaration) = graph.nodes.get(&local.owner) else {
                return Vec::new();
            };
            let end = nearest_kind(local.owner, "block", graph)
                .and_then(|id| graph.nodes.get(&id))
                .map_or(declaration.end_byte(), tree_sitter::Node::end_byte);
            return names
                .into_iter()
                .map(|(name, range)| LocalBinding {
                    name,
                    start_byte: range.start,
                    end_byte: range.end,
                    scope_start: declaration.end_byte(),
                    scope_end: end,
                })
                .collect();
        }
        LocalScopeKind::Loop => child_field(local.owner, "body", graph),
        LocalScopeKind::Condition => condition_scope(local.owner, graph),
        LocalScopeKind::Match => Some(local.owner),
        LocalScopeKind::Closure => child_field(local.owner, "body", graph),
    };
    let Some(scope) = scope.and_then(|id| graph.nodes.get(&id)) else {
        return Vec::new();
    };
    names
        .into_iter()
        .map(|(name, range)| LocalBinding {
            name,
            start_byte: range.start,
            end_byte: range.end,
            scope_start: scope.start_byte(),
            scope_end: scope.end_byte(),
        })
        .collect()
}

fn binding_names(root: usize, graph: &NodeGraph<'_>) -> Vec<(String, Range<usize>)> {
    let mut names = Vec::new();
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        let Some(node) = graph.nodes.get(&id) else {
            continue;
        };
        if (matches!(node.kind(), "identifier" | "self")
            || node.kind() == "shorthand_field_identifier" && is_field(*node, "name"))
            && let Some(name) = graph.text(id)
        {
            names.push((name, node.byte_range()));
        }
        let mut cursor = node.walk();
        pending.extend(node.children(&mut cursor).filter_map(|child| {
            (!is_field(child, "type") && !is_field(child, "condition")).then_some(child.id())
        }));
    }
    names
}

fn is_field(node: tree_sitter::Node<'_>, field: &str) -> bool {
    node.parent()
        .and_then(|parent| parent.child_by_field_name(field))
        .is_some_and(|first| first.id() == node.id())
}

fn nearest_kind(mut id: usize, kind: &str, graph: &NodeGraph<'_>) -> Option<usize> {
    loop {
        if graph.nodes.get(&id).is_some_and(|node| node.kind() == kind) {
            return Some(id);
        }
        id = graph.nodes.get(&id)?.parent()?.id();
    }
}

fn condition_scope(mut id: usize, graph: &NodeGraph<'_>) -> Option<usize> {
    loop {
        let node = graph.nodes.get(&id)?;
        match node.kind() {
            "if_expression" => return child_field(id, "consequence", graph),
            "while_expression" => return child_field(id, "body", graph),
            _ => id = node.parent()?.id(),
        }
    }
}

fn contains(outer: &Range<usize>, inner: &Range<usize>) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

fn relative_module(position: usize, modules: &[ModuleSpan]) -> ModulePath {
    let mut modules: Vec<_> = modules
        .iter()
        .filter(|module| module.body.start <= position && position <= module.body.end)
        .collect();
    modules.sort_by_key(|module| (module.body.end - module.body.start, module.body.start));
    modules.reverse();
    ModulePath(
        modules
            .into_iter()
            .map(|module| module.name.clone())
            .collect(),
    )
}

fn lexical_scope(
    node_id: usize,
    skip_self: bool,
    graph: &NodeGraph<'_>,
    root: &Range<usize>,
) -> (usize, usize) {
    let mut current = graph.nodes.get(&node_id).and_then(|node| {
        if skip_self {
            node.parent().map(|parent| parent.id())
        } else {
            Some(node_id)
        }
    });
    while let Some(id) = current {
        if let Some(node) = graph.nodes.get(&id) {
            if matches!(node.kind(), "block" | "declaration_list" | "source_file") {
                return (node.start_byte(), node.end_byte());
            }
            current = node.parent().map(|parent| parent.id());
        } else {
            break;
        }
    }
    (root.start, root.end)
}

fn deduplicate<T, K: Eq + std::hash::Hash>(values: &mut Vec<T>, key: impl Fn(&T) -> K) {
    let mut seen = HashSet::new();
    values.retain(|value| seen.insert(key(value)));
}
