use std::ops::Range;

use super::{LocalScopeKind, ModuleSpan, NodeGraph, RawLocal};
use crate::model::{LocalBinding, ModulePath};

pub(super) fn local_bindings(local: &RawLocal, graph: &NodeGraph<'_>) -> Vec<LocalBinding> {
    let names = local.pattern.map_or_else(
        || {
            graph
                .node(local.owner)
                .map(|node| vec![("self".to_owned(), node.byte_range())])
                .unwrap_or_default()
        },
        |pattern| binding_names(pattern, graph),
    );
    let scope = match local.scope {
        LocalScopeKind::Function => nearest_kind(local.owner, "function_item", graph)
            .and_then(|id| graph.child_by_field(id, "body"))
            .and_then(|id| graph.range(id)),
        LocalScopeKind::Block => {
            let Some(declaration) = graph.node(local.owner) else {
                return Vec::new();
            };
            let end = nearest_kind(local.owner, "block", graph)
                .and_then(|id| graph.node(id))
                .map_or(declaration.end_byte(), |node| node.end_byte());
            Some(declaration.end_byte()..end)
        }
        LocalScopeKind::Condition => {
            condition_scope(local.owner, graph).and_then(|id| graph.range(id))
        }
        LocalScopeKind::Match => graph.range(local.owner),
        LocalScopeKind::Loop | LocalScopeKind::Closure => graph
            .child_by_field(local.owner, "body")
            .and_then(|id| graph.range(id)),
    };
    let Some(scope) = scope else {
        return Vec::new();
    };
    names
        .into_iter()
        .map(|(name, range)| LocalBinding {
            name,
            start_byte: range.start,
            end_byte: range.end,
            scope_start: scope.start,
            scope_end: scope.end,
            context_id: 0,
        })
        .collect()
}

pub(super) fn binding_names(root: usize, graph: &NodeGraph<'_>) -> Vec<(String, Range<usize>)> {
    let mut names = Vec::new();
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        let Some(node) = graph.node(id) else {
            continue;
        };
        if (matches!(node.kind(), "identifier" | "self")
            || node.kind() == "shorthand_field_identifier" && graph.is_field(id, "name"))
            && let Some(name) = graph.text(id)
        {
            names.push((name, node.byte_range()));
        }
        let mut cursor = node.walk();
        pending.extend(
            node.children(&mut cursor)
                .map(|child| child.id())
                .filter(|child| {
                    !graph.is_field(*child, "type") && !graph.is_field(*child, "condition")
                }),
        );
    }
    names
}

fn nearest_kind(id: usize, kind: &str, graph: &NodeGraph<'_>) -> Option<usize> {
    graph.nearest(id, |_, node| node.kind() == kind)
}

fn condition_scope(id: usize, graph: &NodeGraph<'_>) -> Option<usize> {
    let condition = graph.nearest(id, |_, node| {
        matches!(node.kind(), "if_expression" | "while_expression")
    })?;
    match graph.kind(condition)? {
        "if_expression" => graph.child_by_field(condition, "consequence"),
        "while_expression" => graph.child_by_field(condition, "body"),
        _ => None,
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
    let start = if skip_self {
        graph.parent(node_id).unwrap_or(node_id)
    } else {
        node_id
    };
    graph
        .nearest(start, |_, node| {
            matches!(node.kind(), "block" | "declaration_list" | "source_file")
        })
        .and_then(|id| graph.range(id))
        .map_or((root.start, root.end), |range| (range.start, range.end))
}
