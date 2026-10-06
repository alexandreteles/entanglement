use std::ops::Range;

use tree_sitter::Node;

use crate::model::LocalBinding;

use super::super::SemanticFacts;
use super::super::syntax::{ancestors, body, field, nodes, range, target_names};

pub(super) fn add_target_bindings(
    node: Node<'_>,
    target: Node<'_>,
    root: Node<'_>,
    source: &[u8],
    facts: &mut SemanticFacts,
    excluded: &mut Vec<Range<usize>>,
    visibility_start: usize,
) {
    let names = target_names(target, source);
    excluded.extend(names.iter().map(|(_, span)| span.clone()));
    add_names(node, names, root, facts, visibility_start);
}

pub(super) fn add_names(
    node: Node<'_>,
    names: Vec<(String, Range<usize>)>,
    root: Node<'_>,
    facts: &mut SemanticFacts,
    visibility_start: usize,
) {
    let start = binding_scope_owner(node).map_or(root.start_byte(), |_| visibility_start);
    let scopes = binding_scopes(node, root, start);
    for (name, name_range) in names {
        for scope in &scopes {
            push_local(facts, name.clone(), name_range.clone(), scope.clone());
        }
    }
}

fn binding_scopes(node: Node<'_>, root: Node<'_>, start: usize) -> Vec<Range<usize>> {
    match binding_scope_owner(node) {
        Some(owner) if owner.kind() == "class_definition" => class_segments(owner, start),
        Some(owner) => vec![range(body(owner))],
        None => vec![range(root)],
    }
}

pub(super) fn binding_scope_owner(node: Node<'_>) -> Option<Node<'_>> {
    ancestors(node).find(|item| {
        matches!(item.kind(), "function_definition" | "class_definition")
            && range(body(*item)).start <= node.start_byte()
            && node.end_byte() <= range(body(*item)).end
    })
}

pub(crate) fn class_segments(class: Node<'_>, start: usize) -> Vec<Range<usize>> {
    let class_body = body(class);
    let mut holes = nodes(class_body)
        .into_iter()
        .filter_map(|node| match node.kind() {
            "function_definition" | "class_definition" | "lambda" => Some(
                field(node, "body")
                    .map(range)
                    .unwrap_or_else(|| range(node)),
            ),
            kind if super::comprehensions::is_comprehension(kind) => Some(range(node)),
            _ => None,
        })
        .filter(|item| item.start < class_body.end_byte() && start < item.end)
        .collect::<Vec<_>>();
    holes.sort_by_key(|item| (item.start, item.end));
    let mut cursor = start.max(class_body.start_byte());
    let end = class_body.end_byte();
    let mut segments = Vec::new();
    for hole in holes {
        if cursor < hole.start {
            segments.push(cursor..hole.start.min(end));
        }
        cursor = cursor.max(hole.end);
    }
    if cursor < end {
        segments.push(cursor..end);
    }
    segments
}

pub(super) fn push_local(
    facts: &mut SemanticFacts,
    name: String,
    name_range: Range<usize>,
    scope: Range<usize>,
) {
    if scope.start < scope.end {
        facts.locals.push(LocalBinding {
            name,
            start_byte: name_range.start,
            end_byte: name_range.end,
            scope_start: scope.start,
            scope_end: scope.end,
            context_id: 0,
        });
    }
}
