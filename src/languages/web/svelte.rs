use std::ops::Range;

use tree_sitter::{Language, Node, Parser, QueryCapture, Tree};

use crate::Result;
use crate::languages::{CapturedTree, InjectionRequest, LanguageHandler};
use crate::metrics::cyclomatic::FunctionScope;
use crate::metrics::halstead::HalsteadTokenKind;
use crate::model::{
    Definition, DefinitionKind, Export, LocalBinding, ModulePath, Reference, ReferenceKind,
};

use super::super::query::QueryAnalyzer;

pub(super) fn build(language: &Language) -> Result<Box<dyn LanguageHandler>> {
    let query = QueryAnalyzer::new(language, include_str!("../../queries/svelte.scm"))?;
    Ok(Box::new(Analyzer {
        component_reference: query.capture_id("svelte.component.reference"),
        each_binding: query.capture_id("svelte.each.binding"),
        await_binding: query.capture_id("svelte.await.binding"),
        snippet_parameter: query.capture_id("svelte.snippet.parameter"),
        snippet_definition: query.capture_id("svelte.snippet.definition"),
        declaration: query.capture_id("svelte.declaration"),
        query,
    }))
}

struct Analyzer {
    query: QueryAnalyzer,
    component_reference: Option<u32>,
    each_binding: Option<u32>,
    await_binding: Option<u32>,
    snippet_parameter: Option<u32>,
    snippet_definition: Option<u32>,
    declaration: Option<u32>,
}

impl LanguageHandler for Analyzer {
    fn capture(&self, tree: &Tree, source: &[u8], include_tokens: bool) -> Result<CapturedTree> {
        let root = tree.root_node();
        let range = root.byte_range();
        let mut extras = Extras::default();
        let query = self
            .query
            .capture_with(tree, source, include_tokens, |_, captures, bytes| {
                extras.record(self, captures, bytes, range.clone());
            });
        let mut captured: CapturedTree = query.into();

        configure_injections(root, source, &mut captured.injections);
        captured.functions.push(FunctionScope {
            name: "<component>".into(),
            range: range.clone(),
            line: root.start_position().row + 1,
        });
        captured.definitions.push(component_definition(range.clone()));
        captured.exports.push(component_export(range.clone()));
        captured.definitions.extend(extras.definitions);
        captured.references.extend(extras.references);
        captured.locals.extend(extras.locals);
        classify_tokens(&mut captured);
        captured
            .functions
            .sort_by_key(|function| (function.range.start, function.range.end));
        Ok(captured)
    }
}

#[derive(Default)]
struct Extras {
    definitions: Vec<Definition>,
    references: Vec<Reference>,
    locals: Vec<LocalBinding>,
}

impl Extras {
    fn record(
        &mut self,
        analyzer: &Analyzer,
        captures: &[QueryCapture<'_>],
        source: &[u8],
        root: Range<usize>,
    ) {
        for capture in captures {
            let node = capture.node;
            if Some(capture.index) == analyzer.component_reference {
                self.references.push(component_reference(node, source, &root));
            } else if Some(capture.index) == analyzer.each_binding {
                self.add_pattern(node, source, each_scope(node));
            } else if Some(capture.index) == analyzer.await_binding {
                self.add_pattern(node, source, await_scope(node));
            } else if Some(capture.index) == analyzer.snippet_parameter {
                self.add_pattern(node, source, snippet_scope(node));
            } else if Some(capture.index) == analyzer.snippet_definition {
                self.add_snippet(node, source, &root);
            } else if Some(capture.index) == analyzer.declaration {
                self.add_declaration(node, source, declaration_scope(node, &root));
            }
        }
    }

    fn add_pattern(&mut self, node: Node<'_>, source: &[u8], scope: Option<Range<usize>>) {
        self.add_names(node, parse_names(node, source, false), scope);
    }

    fn add_declaration(&mut self, node: Node<'_>, source: &[u8], scope: Option<Range<usize>>) {
        self.add_names(node, parse_names(node, source, true), scope);
    }

    fn add_names(&mut self, node: Node<'_>, names: Vec<String>, scope: Option<Range<usize>>) {
        let Some(scope) = scope else { return };
        self.locals.extend(names.into_iter().map(|name| LocalBinding {
            name,
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
            scope_start: scope.start,
            scope_end: scope.end,
            context_id: 0,
        }));
    }

    fn add_snippet(&mut self, name: Node<'_>, source: &[u8], root: &Range<usize>) {
        let Some(block) = ancestor(name, "snippet_block") else {
            return;
        };
        let scope = lexical_parent_scope(block, root);
        self.definitions.push(Definition {
            name: text(name, source),
            kind: DefinitionKind::Function,
            module: ModulePath::default(),
            start_byte: block.start_byte(),
            end_byte: block.end_byte(),
            scope_start: scope.start,
            scope_end: scope.end,
            is_public: false,
            inline_module: false,
            external_module: false,
            context_id: 0,
        });
    }
}

fn configure_injections(root: Node<'_>, source: &[u8], requests: &mut [InjectionRequest]) {
    for request in requests {
        if !matches!(request.language.as_str(), "javascript" | "typescript") {
            continue;
        }
        let script = root
            .descendant_for_byte_range(request.range.start, request.range.end)
            .and_then(|node| ancestor_or_self(node, "element"))
            .filter(|element| element_tag(*element, source).as_deref() == Some("script"));
        if script.is_some_and(|element| is_module_script(element, source)) {
            request.share_bindings = true;
        } else {
            request.inherit_scope = true;
            request.inherit_metrics = true;
            request.inherit_context = true;
            request.publish_exports = false;
        }
    }
}

fn is_module_script(element: Node<'_>, source: &[u8]) -> bool {
    let Some(tag) = named_children(element).find(|node| node.kind() == "start_tag") else {
        return false;
    };
    named_children(tag)
        .filter(|node| node.kind() == "attribute")
        .any(|attribute| {
            let name = attribute
                .child_by_field_name("name")
                .map(|node| text(node, source));
            if name.as_deref() == Some("module") {
                return true;
            }
            name.as_deref() == Some("context")
                && attribute
                    .child_by_field_name("value")
                    .map(|node| text(node, source))
                    .is_some_and(|value| trim_quotes(&value) == "module")
        })
}

fn element_tag(element: Node<'_>, source: &[u8]) -> Option<String> {
    let tag = named_children(element)
        .find(|node| matches!(node.kind(), "start_tag" | "self_closing_tag"))?;
    tag.child_by_field_name("name")
        .map(|node| text(node, source))
        .or_else(|| {
            named_children(tag)
                .find(|node| node.kind() == "tag_name")
                .map(|node| text(node, source))
        })
}

fn trim_quotes(value: &str) -> &str {
    value
        .trim()
        .trim_matches(|character| matches!(character, '\'' | '"'))
}

fn parse_names(node: Node<'_>, source: &[u8], declaration: bool) -> Vec<String> {
    let body = text(node, source);
    let typed = node
        .named_child(0)
        .is_some_and(|child| child.kind() == "ts");
    let wrapped = if declaration {
        format!("let {body};")
    } else {
        format!("function __entanglement({body}) {{}}")
    };
    let language: Language = if typed {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    } else {
        tree_sitter_javascript::LANGUAGE.into()
    };
    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(&wrapped, None) else {
        return Vec::new();
    };
    let kind = if declaration {
        "variable_declarator"
    } else {
        "formal_parameters"
    };
    let Some(pattern) = find_kind(tree.root_node(), kind).and_then(|node| {
        declaration
            .then(|| node.child_by_field_name("name"))
            .flatten()
            .or(Some(node))
    }) else {
        return Vec::new();
    };
    super::facts::binding_names(pattern, wrapped.as_bytes())
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

fn find_kind(node: Node<'_>, kind: &str) -> Option<Node<'_>> {
    if node.kind() == kind {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find_map(|child| find_kind(child, kind))
}

fn component_definition(range: Range<usize>) -> Definition {
    Definition {
        name: "<component>".into(),
        kind: DefinitionKind::Other,
        module: ModulePath::default(),
        start_byte: range.start,
        end_byte: range.end,
        scope_start: range.start,
        scope_end: range.end,
        is_public: true,
        inline_module: false,
        external_module: false,
        context_id: 0,
    }
}

fn component_export(range: Range<usize>) -> Export {
    Export {
        source: None,
        imported_name: None,
        local_name: Some("<component>".into()),
        exported_name: "default".into(),
        namespace: false,
        start_byte: range.start,
        end_byte: range.end,
        scope_start: range.start,
        scope_end: range.end,
        context_id: 0,
    }
}

fn component_reference(node: Node<'_>, source: &[u8], root: &Range<usize>) -> Reference {
    Reference {
        path: text(node, source).split('.').map(str::to_owned).collect(),
        module: ModulePath::default(),
        kind: ReferenceKind::Value,
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        scope_start: root.start,
        scope_end: root.end,
        call_owner: Some(root.start),
        context_id: 0,
    }
}

fn each_scope(node: Node<'_>) -> Option<Range<usize>> {
    let block = ancestor(node, "each_block")?;
    let end = named_children(block)
        .find(|child| child.kind() == "else_clause")
        .map_or(block.end_byte(), |child| child.start_byte());
    Some(node.end_byte()..end)
}

fn await_scope(node: Node<'_>) -> Option<Range<usize>> {
    if let Some(branch) = ancestor(node, "await_branch") {
        return Some(node.end_byte()..branch.end_byte());
    }
    let block = ancestor(node, "await_block")?;
    let end = named_children(block)
        .find(|child| child.kind() == "await_branch" && child.start_byte() > node.end_byte())
        .map_or(block.end_byte(), |child| child.start_byte());
    Some(node.end_byte()..end)
}

fn snippet_scope(node: Node<'_>) -> Option<Range<usize>> {
    let block = ancestor(node, "snippet_block")?;
    Some(node.end_byte()..block.end_byte())
}

fn declaration_scope(node: Node<'_>, root: &Range<usize>) -> Option<Range<usize>> {
    let scope = lexical_parent_scope(node, root);
    (node.end_byte() < scope.end).then_some(node.end_byte()..scope.end)
}

fn lexical_parent_scope(node: Node<'_>, root: &Range<usize>) -> Range<usize> {
    let mut current = node.parent();
    while let Some(item) = current {
        if matches!(
            item.kind(),
            "snippet_block"
                | "if_block"
                | "each_block"
                | "await_block"
                | "await_branch"
                | "key_block"
                | "document"
        ) {
            return item.byte_range();
        }
        current = item.parent();
    }
    root.clone()
}

fn ancestor<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut current = node.parent();
    while let Some(item) = current {
        if item.kind() == kind {
            return Some(item);
        }
        current = item.parent();
    }
    None
}

fn ancestor_or_self<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    (node.kind() == kind)
        .then_some(node)
        .or_else(|| ancestor(node, kind))
}

fn named_children(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .collect::<Vec<_>>()
        .into_iter()
}

fn text(node: Node<'_>, source: &[u8]) -> String {
    String::from_utf8_lossy(&source[node.byte_range()]).into_owned()
}

fn classify_tokens(captured: &mut CapturedTree) {
    for token in &mut captured.tokens {
        if matches!(
            token.token.as_str(),
            "{#" | "{:" | "{@" | "{/" | "}" | "if" | "else" | "each" | "await"
                | "then" | "catch" | "key" | "snippet" | "render" | "attach" | "html"
                | "debug" | "const"
        ) {
            token.kind = HalsteadTokenKind::Operator;
        }
    }
}
