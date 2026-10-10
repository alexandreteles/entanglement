use std::ops::Range;

use tree_sitter::{Node, QueryCapture};

use crate::languages::CapturedTree;
use crate::metrics::SyntaxRole;
use crate::model::{Definition, DefinitionKind, Export, ModulePath, Reference, ReferenceKind};

use super::Analyzer;
use super::scopes::{call_owner, lexical_scope};

#[derive(Default)]
pub(super) struct Extras {
    pub frontmatter_start: Option<usize>,
    component_references: Vec<Reference>,
}

impl Extras {
    pub(super) fn record(
        &mut self,
        analyzer: &Analyzer,
        captures: &[QueryCapture<'_>],
        source: &[u8],
        root: &Range<usize>,
    ) {
        for capture in captures {
            if Some(capture.index) == analyzer.component_reference {
                self.component_references
                    .push(component_reference(capture.node, source, root));
            } else if Some(capture.index) == analyzer.frontmatter {
                self.frontmatter_start = Some(
                    self.frontmatter_start
                        .map_or(capture.node.start_byte(), |start| {
                            start.min(capture.node.start_byte())
                        }),
                );
            }
        }
    }
}

pub(super) fn finish(extras: &mut Extras, cutoff: usize, captured: &mut CapturedTree) {
    captured.references.append(&mut extras.component_references);
    captured
        .events
        .retain(|event| event.start_byte >= cutoff || is_source_line_event(event.role));
    captured
        .functions
        .retain(|function| function.range.start >= cutoff);
    captured
        .definitions
        .retain(|item| item.start_byte >= cutoff);
    captured.imports.retain(|item| item.start_byte >= cutoff);
    captured.exports.retain(|item| item.start_byte >= cutoff);
    let astro_scopes = astro_scopes(captured);
    captured.references.retain(|item| {
        item.start_byte >= cutoff
            && (astro_is_bound(&astro_scopes, item.start_byte)
                || item.path.len() != 1
                || item.path.first().map(String::as_str) != Some("Astro"))
    });
    captured.locals.retain(|item| item.start_byte >= cutoff);
    captured
        .injections
        .retain(|item| item.range.start >= cutoff);
}

fn astro_scopes(captured: &CapturedTree) -> Vec<Range<usize>> {
    let imports = captured
        .imports
        .iter()
        .filter(|item| item.alias.as_deref().or(item.imported_name.as_deref()) == Some("Astro"))
        .map(|item| item.scope_start..item.scope_end);
    let definitions = captured
        .definitions
        .iter()
        .filter(|item| item.name == "Astro")
        .map(|item| item.scope_start..item.scope_end);
    let locals = captured
        .locals
        .iter()
        .filter(|item| item.name == "Astro")
        .map(|item| item.scope_start..item.scope_end);
    imports.chain(definitions).chain(locals).collect()
}

fn astro_is_bound(scopes: &[Range<usize>], position: usize) -> bool {
    scopes
        .iter()
        .any(|scope| scope.start <= position && position < scope.end)
}

fn is_source_line_event(role: SyntaxRole) -> bool {
    matches!(role, SyntaxRole::Node | SyntaxRole::Comment)
}

fn component_reference(node: Node<'_>, source: &[u8], root: &Range<usize>) -> Reference {
    let name = String::from_utf8_lossy(&source[node.byte_range()]);
    let path = if name == "Astro.self" {
        vec!["<component>".to_owned()]
    } else {
        name.split('.').map(str::to_owned).collect()
    };
    let scope = lexical_scope(node, root);
    Reference {
        path,
        module: ModulePath::default(),
        kind: ReferenceKind::Value,
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        scope_start: scope.start,
        scope_end: scope.end,
        call_owner: Some(call_owner(node, root)),
        context_id: 0,
    }
}

pub(super) fn component_definition(range: Range<usize>) -> Definition {
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

pub(super) fn component_export(range: Range<usize>) -> Export {
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
