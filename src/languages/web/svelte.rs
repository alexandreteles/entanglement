use std::ops::Range;

use tree_sitter::{Language, Node, QueryCapture, Tree};

use crate::Result;
use crate::languages::{CapturedTree, InjectionRequest, LanguageHandler};
use crate::metrics::cyclomatic::FunctionScope;
use crate::metrics::halstead::HalsteadTokenKind;
use crate::model::{
    Definition, DefinitionKind, Export, LocalBinding, ModulePath, Reference, ReferenceKind,
};

use super::super::query::QueryAnalyzer;
use bindings::parse_names;
use scopes::{
    ancestor, await_scope, declaration_scope, each_scope, lexical_parent_scope, snippet_scope,
};

mod bindings;
mod scopes;

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
        captured
            .definitions
            .push(component_definition(range.clone()));
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
                self.references
                    .push(component_reference(node, source, &root));
            } else if Some(capture.index) == analyzer.each_binding {
                self.add_pattern(node, source, each_scope(node));
            } else if Some(capture.index) == analyzer.await_binding {
                self.add_pattern(node, source, await_scope(node));
            } else if Some(capture.index) == analyzer.snippet_parameter {
                self.add_pattern(node, source, snippet_scope(node));
            } else if Some(capture.index) == analyzer.snippet_definition {
                self.add_snippet(node, source, &root);
            } else if Some(capture.index) == analyzer.declaration {
                self.add_declaration(node, source, Some(declaration_scope(node, &root)));
            }
        }
    }

    fn add_pattern(&mut self, node: Node<'_>, source: &[u8], scope: Option<Range<usize>>) {
        self.add_names(node, parse_names(node, source, false), scope);
    }

    fn add_declaration(&mut self, node: Node<'_>, source: &[u8], scope: Option<Range<usize>>) {
        // Keep the host binding outside the guest expression's excluded range.
        let owner = ancestor(node, "const_tag")
            .or_else(|| ancestor(node, "declaration_tag"))
            .unwrap_or(node);
        self.add_names(owner, parse_names(node, source, true), scope);
    }

    fn add_names(&mut self, node: Node<'_>, names: Vec<String>, scope: Option<Range<usize>>) {
        let Some(scope) = scope else { return };
        self.locals
            .extend(names.into_iter().map(|name| LocalBinding {
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
            "{#" | "{:"
                | "{@"
                | "{/"
                | "}"
                | "if"
                | "else"
                | "each"
                | "await"
                | "then"
                | "catch"
                | "key"
                | "snippet"
                | "render"
                | "attach"
                | "html"
                | "debug"
                | "const"
        ) {
            token.kind = HalsteadTokenKind::Operator;
        }
    }
}
