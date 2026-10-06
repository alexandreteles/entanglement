use std::ops::Range;

use super::references::child_field;
use super::{LocalScopeKind, ModuleSpan, NodeGraph, RawLocal};
use crate::model::{LocalBinding, ModulePath};

pub(super) fn local_bindings(local: &RawLocal, graph: &NodeGraph<'_>) -> Vec<LocalBinding> {
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
                    context_id: 0,
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
            context_id: 0,
        })
        .collect()
}

pub(super) fn binding_names(root: usize, graph: &NodeGraph<'_>) -> Vec<(String, Range<usize>)> {
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

pub(super) fn contains(outer: &Range<usize>, inner: &Range<usize>) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

pub(super) fn relative_module(position: usize, modules: &[ModuleSpan]) -> ModulePath {
    ModulePath(
        modules
            .iter()
            .filter(|module| module.body.start <= position && position <= module.body.end)
            .map(|module| module.name.clone())
            .collect(),
    )
}

pub(super) fn lexical_scope(
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
