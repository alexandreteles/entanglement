use std::ops::Range;

use tree_sitter::Node;

use super::super::SemanticFacts;
use super::super::syntax::{children, field, range, target_names, text};
use super::scope::{add_names, add_target_bindings, binding_scope_owner};

pub(super) fn collect_with_target(
    item: Node<'_>,
    root: Node<'_>,
    source: &[u8],
    facts: &mut SemanticFacts,
    excluded: &mut Vec<Range<usize>>,
) {
    let mut pending = children(item);
    while let Some(node) = pending.pop() {
        if node.kind() == "as_pattern" {
            if let Some(alias) = field(node, "alias") {
                add_target_bindings(item, alias, root, source, facts, excluded, item.end_byte());
            }
        } else {
            pending.extend(children(node));
        }
    }
}

pub(super) fn collect_named_expression(
    node: Node<'_>,
    root: Node<'_>,
    source: &[u8],
    facts: &mut SemanticFacts,
    excluded: &mut Vec<Range<usize>>,
) {
    let Some(name_node) = field(node, "name") else {
        return;
    };
    let name_range = range(name_node);
    excluded.push(name_range.clone());
    add_names(
        node,
        vec![(text(name_node, source), name_range)],
        root,
        facts,
        node.end_byte(),
    );
}

pub(super) fn collect_scope_declaration(
    declaration: Node<'_>,
    root: Node<'_>,
    source: &[u8],
    facts: &mut SemanticFacts,
    excluded: &mut Vec<Range<usize>>,
) {
    let names = children(declaration)
        .into_iter()
        .filter(|node| node.kind() == "identifier")
        .map(|node| (text(node, source), range(node)))
        .collect::<Vec<_>>();
    excluded.extend(names.iter().map(|(_, span)| span.clone()));
    add_names(declaration, names, root, facts, declaration.start_byte());
}

pub(super) fn collect_deletion(
    statement: Node<'_>,
    root: Node<'_>,
    source: &[u8],
    facts: &mut SemanticFacts,
    excluded: &mut Vec<Range<usize>>,
) {
    let names = children(statement)
        .into_iter()
        .flat_map(|target| {
            if matches!(
                target.kind(),
                "identifier" | "tuple_pattern" | "list_pattern"
            ) {
                target_names(target, source)
            } else {
                Vec::new()
            }
        })
        .collect::<Vec<_>>();
    excluded.extend(names.iter().map(|(_, span)| span.clone()));
    add_names(statement, names, root, facts, statement.end_byte());
}

pub(super) fn collect_definition(
    node: Node<'_>,
    root: Node<'_>,
    source: &[u8],
    facts: &mut SemanticFacts,
    excluded: &mut Vec<Range<usize>>,
) {
    if !binding_scope_owner(node).is_some_and(|owner| owner.kind() == "class_definition") {
        return;
    }
    let Some(name) = field(node, "name") else {
        return;
    };
    let name_range = range(name);
    excluded.push(name_range.clone());
    add_names(
        node,
        vec![(text(name, source), name_range)],
        root,
        facts,
        node.end_byte(),
    );
}
