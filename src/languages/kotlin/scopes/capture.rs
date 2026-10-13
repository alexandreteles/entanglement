use std::ops::Range;

use tree_sitter::Node;

use crate::metrics::cyclomatic::FunctionScope;

use super::{
    ScopeIndex,
    names::{executable_scope_name, recovered_error},
    nodes::*,
    syntax::{self, ContextParameters},
};

/// Return every syntactically executable Kotlin range. Bodyless declarations
/// stay out so expect/abstract/external APIs do not receive a synthetic
/// complexity baseline.
pub(in crate::languages::kotlin) fn capture(
    index: &ScopeIndex<'_>,
    query_functions: Vec<FunctionScope>,
) -> Vec<FunctionScope> {
    let root = index.root();
    let source = index.source();
    let mut scopes = Vec::new();
    let mut pending = vec![root];
    let mut script_nodes = Vec::new();
    let recovered_types = index.recovered_types();

    while let Some(node) = pending.pop() {
        let name = executable_scope_name(node, source, &query_functions, recovered_types);
        if let Some(name) = name {
            let start = syntax::context_start(node, index.contexts());
            scopes.push(scope(name, start, node.end_byte(), source));
        }

        if node.kind() == "source_file" {
            for index in 0..node.named_child_count() {
                if let Some(child) = node.named_child(index as u32)
                    && is_top_level_executable(child.kind())
                    && !recovered_error(child, recovered_types)
                {
                    script_nodes.push(child);
                }
            }
        }

        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                pending.push(child);
            }
        }
    }

    if let (Some(first), Some(last)) = (script_nodes.first(), script_nodes.last()) {
        let start = first.start_byte();
        let end = last.end_byte();
        scopes.push(scope(
            format!("script@{}", first.start_position().row + 1),
            start,
            end,
            source,
        ));
    }

    scopes.sort_by_key(|item| (item.range.start, item.range.end, item.name.clone()));
    scopes.dedup_by(|left, right| left.range == right.range && left.name == right.name);
    scopes
}
/// Return the nearest captured metric scope containing `node`.
///
/// The returned identity is exactly the byte range used by `FunctionScope`.
/// Consumers attaching semantic calls must still exclude source that executes
/// at call sites (such as default arguments) from body-call ownership.
pub(super) fn metric_scope_range_for(
    index: &ScopeIndex<'_>,
    node: Node<'_>,
) -> Option<Range<usize>> {
    let root = index.root();
    let recovered_types = index.recovered_types();
    let mut current = Some(node);
    while let Some(candidate) = current {
        if executable_scope_name(candidate, index.source(), &[], recovered_types).is_some() {
            return Some(syntax::context_start(candidate, index.contexts())..candidate.end_byte());
        }
        if candidate.id() == root.id() {
            break;
        }
        current = candidate.parent();
    }

    let mut direct_child = node;
    while let Some(parent) = direct_child.parent() {
        if parent.id() == root.id() {
            break;
        }
        direct_child = parent;
    }
    if direct_child
        .parent()
        .is_none_or(|parent| parent.id() != root.id())
        || !is_top_level_executable(direct_child.kind())
    {
        return None;
    }

    let script_nodes = (0..root.named_child_count())
        .filter_map(|index| root.named_child(index as u32))
        .filter(|child| is_top_level_executable(child.kind()))
        .filter(|child| !recovered_error(*child, recovered_types))
        .collect::<Vec<_>>();
    let first = script_nodes.first()?;
    let last = script_nodes.last()?;
    Some(first.start_byte()..last.end_byte())
}
pub(in crate::languages::kotlin) fn property_initializer_range(
    property: Node<'_>,
    contexts: &[ContextParameters],
) -> Option<Range<usize>> {
    (property.kind() == "property_declaration"
        && has_property_initializer(property)
        && bound_function_value(property).is_none())
    .then(|| syntax::context_start(property, contexts)..property.end_byte())
}
