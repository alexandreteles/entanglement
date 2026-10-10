use std::ops::Range;

use super::super::capture::has_visibility;
use super::super::scopes::{lexical_scope, relative_module};
use super::super::{CaptureFacts, ModuleSpan, NodeGraph, RawImport};
use super::paths::{child_field, is_path_node, is_use_node, path_segments};
use crate::model::Import;

impl CaptureFacts<'_> {
    pub(in crate::languages::rust) fn normalize_imports(
        &self,
        modules: &[ModuleSpan],
        root: &Range<usize>,
    ) -> Vec<Import> {
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
                    source: None,
                    imported_name: None,
                    namespace: false,
                    start_byte: import.range.start,
                    end_byte: import.range.end,
                    module,
                    scope_start,
                    scope_end,
                    is_public: import.is_public,
                    context_id: 0,
                }
            })
            .collect()
    }
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
        "scoped_use_list" => flatten_scoped_use_list(node_id, prefix, is_public, graph, imports),
        "use_list" => flatten_use_list(node, prefix, is_public, graph, imports),
        "use_as_clause" => {
            let path = field_path(node_id, prefix, graph);
            let alias = child_field(node_id, "alias", graph).and_then(|alias| graph.text(alias));
            push_import(node_id, node, path, alias, is_public, imports);
        }
        "use_wildcard" => flatten_wildcard(node, prefix, is_public, graph, imports),
        "self" if !prefix.is_empty() => {
            push_import(node_id, node, prefix.to_vec(), None, is_public, imports);
        }
        _ => {
            let mut path = prefix.to_vec();
            path.extend(path_segments(node_id, graph));
            push_import(node_id, node, path, None, is_public, imports);
        }
    }
}

fn flatten_scoped_use_list(
    node_id: usize,
    prefix: &[String],
    is_public: bool,
    graph: &NodeGraph<'_>,
    imports: &mut Vec<RawImport>,
) {
    let path = field_path(node_id, prefix, graph);
    if let Some(list) = child_field(node_id, "list", graph) {
        flatten_use_node(list, &path, is_public, graph, imports);
    }
}

fn flatten_use_list(
    node: tree_sitter::Node<'_>,
    prefix: &[String],
    is_public: bool,
    graph: &NodeGraph<'_>,
    imports: &mut Vec<RawImport>,
) {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| is_use_node(child.id(), graph))
        .for_each(|child| flatten_use_node(child.id(), prefix, is_public, graph, imports));
}

fn flatten_wildcard(
    node: tree_sitter::Node<'_>,
    prefix: &[String],
    is_public: bool,
    graph: &NodeGraph<'_>,
    imports: &mut Vec<RawImport>,
) {
    let mut path = prefix.to_vec();
    let mut cursor = node.walk();
    path.extend(
        node.children(&mut cursor)
            .filter(|child| is_path_node(child.id(), graph))
            .flat_map(|child| path_segments(child.id(), graph)),
    );
    path.push("*".into());
    push_import(node.id(), node, path, None, is_public, imports);
}

fn field_path(node_id: usize, prefix: &[String], graph: &NodeGraph<'_>) -> Vec<String> {
    let mut path = prefix.to_vec();
    if let Some(path_node) = child_field(node_id, "path", graph) {
        path.extend(path_segments(path_node, graph));
    }
    path
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
