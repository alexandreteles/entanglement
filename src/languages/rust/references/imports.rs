use std::ops::Range;
use tree_sitter::Node;

use super::super::capture::has_visibility;
use super::super::scopes::{lexical_scope, relative_module};
use super::super::{CaptureFacts, ModuleSpan, RawImport, node_text};
use super::paths::{is_path_node, is_use_node, path_segments};
use crate::model::Import;

impl CaptureFacts<'_> {
    pub(in crate::languages::rust) fn normalize_imports(
        &self,
        modules: &[ModuleSpan],
        root: &Range<usize>,
    ) -> Vec<Import> {
        let mut raw = Vec::new();
        for node in &self.import_roots {
            if let Some(argument) = node.child_by_field_name("argument") {
                flatten_use_node(argument, &[], has_visibility(*node), self.source, &mut raw);
            }
        }
        raw.into_iter()
            .map(|import| {
                let (scope_start, scope_end) = lexical_scope(import.node, false, root);
                Import {
                    path: import.path,
                    alias: import.alias,
                    source: None,
                    imported_name: None,
                    namespace: false,
                    start_byte: import.node.start_byte(),
                    end_byte: import.node.end_byte(),
                    module: relative_module(import.node.start_byte(), modules),
                    scope_start,
                    scope_end,
                    is_public: import.is_public,
                    context_id: 0,
                }
            })
            .collect()
    }
}

fn flatten_use_node<'tree>(
    node: Node<'tree>,
    prefix: &[String],
    is_public: bool,
    source: &[u8],
    imports: &mut Vec<RawImport<'tree>>,
) {
    match node.kind() {
        "scoped_use_list" => {
            let path = field_path(node, prefix, source);
            if let Some(list) = node.child_by_field_name("list") {
                flatten_use_node(list, &path, is_public, source, imports);
            }
        }
        "use_list" => {
            let mut cursor = node.walk();
            node.children(&mut cursor)
                .filter(|child| is_use_node(*child))
                .for_each(|child| flatten_use_node(child, prefix, is_public, source, imports));
        }
        "use_as_clause" => {
            let path = field_path(node, prefix, source);
            let alias = node
                .child_by_field_name("alias")
                .map(|alias| node_text(alias, source));
            push_import(node, path, alias, is_public, imports);
        }
        "use_wildcard" => {
            let mut path = prefix.to_vec();
            let mut cursor = node.walk();
            path.extend(
                node.children(&mut cursor)
                    .filter(|child| is_path_node(*child))
                    .flat_map(|child| path_segments(child, source)),
            );
            path.push("*".into());
            push_import(node, path, None, is_public, imports);
        }
        "self" if !prefix.is_empty() => {
            push_import(node, prefix.to_vec(), None, is_public, imports)
        }
        _ => {
            let mut path = prefix.to_vec();
            path.extend(path_segments(node, source));
            push_import(node, path, None, is_public, imports);
        }
    }
}

fn field_path(node: Node<'_>, prefix: &[String], source: &[u8]) -> Vec<String> {
    let mut path = prefix.to_vec();
    if let Some(path_node) = node.child_by_field_name("path") {
        path.extend(path_segments(path_node, source));
    }
    path
}

fn push_import<'tree>(
    node: Node<'tree>,
    path: Vec<String>,
    alias: Option<String>,
    is_public: bool,
    imports: &mut Vec<RawImport<'tree>>,
) {
    if !path.is_empty() {
        imports.push(RawImport {
            node,
            path,
            alias,
            is_public,
        });
    }
}
