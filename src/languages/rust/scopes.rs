use std::ops::Range;
use tree_sitter::Node;

use super::{LocalScopeKind, ModuleSpan, RawLocal, node_text};
use crate::languages::syntax::ancestors;
use crate::model::{LocalBinding, ModulePath};

pub(super) fn local_bindings(local: &RawLocal<'_>) -> Vec<LocalBinding> {
    let scope = match local.scope {
        LocalScopeKind::Function => nearest_kind(local.owner, "function_item")
            .and_then(|node| node.child_by_field_name("body"))
            .map(|node| node.byte_range()),
        LocalScopeKind::Block => {
            let end = nearest_kind(local.owner, "block")
                .map_or(local.owner.end_byte(), |node| node.end_byte());
            Some(local.owner.end_byte()..end)
        }
        LocalScopeKind::Condition => condition_scope(local.owner).map(|node| node.byte_range()),
        LocalScopeKind::Match => Some(local.owner.byte_range()),
        LocalScopeKind::Loop | LocalScopeKind::Closure => local
            .owner
            .child_by_field_name("body")
            .map(|node| node.byte_range()),
    };
    let Some(scope) = scope else {
        return Vec::new();
    };
    local
        .names
        .iter()
        .map(|(name, range)| LocalBinding {
            name: name.clone(),
            start_byte: range.start,
            end_byte: range.end,
            scope_start: scope.start,
            scope_end: scope.end,
            context_id: 0,
            ..LocalBinding::default()
        })
        .collect()
}

pub(super) fn binding_names(root: Node<'_>, source: &[u8]) -> Vec<(String, Range<usize>)> {
    let mut names = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if matches!(node.kind(), "identifier" | "self")
            || node.kind() == "shorthand_field_identifier" && is_field(node, "name")
        {
            names.push((node_text(node, source), node.byte_range()));
        }
        let mut cursor = node.walk();
        pending.extend(
            node.children(&mut cursor)
                .filter(|child| !is_field(*child, "type") && !is_field(*child, "condition")),
        );
    }
    names
}

pub(super) fn is_field(node: Node<'_>, field: &str) -> bool {
    node.parent()
        .and_then(|parent| parent.child_by_field_name(field))
        == Some(node)
}

fn nearest_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    ancestors(node).find(|node| node.kind() == kind)
}

fn condition_scope(node: Node<'_>) -> Option<Node<'_>> {
    let condition =
        ancestors(node).find(|node| matches!(node.kind(), "if_expression" | "while_expression"))?;
    let field = match condition.kind() {
        "if_expression" => "consequence",
        "while_expression" => "body",
        _ => unreachable!("the selected node is an if or while expression"),
    };
    condition.child_by_field_name(field)
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
    node: Node<'_>,
    skip_self: bool,
    root: &Range<usize>,
) -> (usize, usize) {
    let start = if skip_self {
        node.parent().unwrap_or(node)
    } else {
        node
    };
    ancestors(start)
        .find(|node| matches!(node.kind(), "block" | "declaration_list" | "source_file"))
        .map_or((root.start, root.end), |node| {
            (node.start_byte(), node.end_byte())
        })
}
