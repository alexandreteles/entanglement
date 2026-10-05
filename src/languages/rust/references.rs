use std::ops::Range;

use super::capture::has_visibility;
use super::node_text;
use super::scopes::{binding_names, contains, lexical_scope, relative_module};
use super::{CaptureFacts, ModuleSpan, NodeGraph, RawImport, RawReference};
use crate::model::{Import, Reference, ReferenceKind};

impl<'a> CaptureFacts<'a> {
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

    pub(super) fn normalize_imports(
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

    pub(super) fn normalize_references(
        &mut self,
        modules: &[ModuleSpan],
        root: &Range<usize>,
    ) -> Vec<Reference> {
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
        let mut references = std::mem::take(&mut self.references);
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
                    call_owner: self.call_owner(
                        reference.node_id,
                        &reference.range,
                        reference.kind,
                    ),
                }
            })
            .collect()
    }

    fn call_owner(
        &self,
        node_id: usize,
        reference_range: &Range<usize>,
        kind: ReferenceKind,
    ) -> Option<usize> {
        if !matches!(
            kind,
            ReferenceKind::Call
                | ReferenceKind::Method
                | ReferenceKind::Qualified
                | ReferenceKind::Value
        ) {
            return None;
        }
        let reference = self.graph.nodes.get(&node_id).copied()?;
        let mut current = reference;
        loop {
            match current.kind() {
                "closure_expression" | "async_block" => return None,
                "call_expression" => {
                    let is_direct_target = current
                        .child_by_field_name("function")
                        .is_some_and(|callee| direct_callee(callee, reference, reference_range));
                    if !is_direct_target {
                        return None;
                    }
                }
                "function_item" => return None,
                _ => {
                    current = current.parent()?;
                    continue;
                }
            }

            current = current.parent()?;
            loop {
                match current.kind() {
                    "closure_expression" | "async_block" => return None,
                    "function_item" => return Some(current.start_byte()),
                    _ => current = current.parent()?,
                }
            }
        }
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

fn direct_callee<'tree>(
    mut callee: tree_sitter::Node<'tree>,
    reference: tree_sitter::Node<'tree>,
    reference_range: &Range<usize>,
) -> bool {
    loop {
        if same_reference(callee, reference, reference_range) {
            return true;
        }
        match callee.kind() {
            "generic_function" => {
                let Some(function) = callee.child_by_field_name("function") else {
                    return false;
                };
                callee = function;
            }
            "parenthesized_expression" if callee.named_child_count() == 1 => {
                let Some(expression) = callee.named_child(0) else {
                    return false;
                };
                callee = expression;
            }
            "scoped_identifier" => {
                return callee
                    .child_by_field_name("name")
                    .is_some_and(|name| same_reference(name, reference, reference_range));
            }
            "field_expression" => {
                return callee
                    .child_by_field_name("field")
                    .is_some_and(|field| same_reference(field, reference, reference_range));
            }
            _ => return false,
        }
    }
}

fn same_reference(
    node: tree_sitter::Node<'_>,
    reference: tree_sitter::Node<'_>,
    reference_range: &Range<usize>,
) -> bool {
    node.id() == reference.id()
        && node.start_byte() == reference_range.start
        && node.end_byte() == reference_range.end
}

pub(super) fn raw_reference(
    node: tree_sitter::Node<'_>,
    kind: ReferenceKind,
    source: &[u8],
) -> RawReference {
    RawReference {
        node_id: node.id(),
        path: vec![node_text(node, source)],
        range: node.start_byte()..node.end_byte(),
        kind,
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

pub(super) fn child_field(node_id: usize, field: &str, graph: &NodeGraph<'_>) -> Option<usize> {
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
