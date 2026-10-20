use std::{collections::HashMap, ops::Range};

use tree_sitter::Node;

use super::syntax::{self, ContextParameters};

mod names;
use names::*;

use crate::metrics::cyclomatic::FunctionScope;

pub(super) struct ScopeIndex<'tree> {
    root: Node<'tree>,
    source: &'tree [u8],
    contexts: Vec<ContextParameters>,
    recovered_types: Vec<syntax::RecoveredType>,
    nodes: Vec<Node<'tree>>,
    executable: HashMap<usize, Range<usize>>,
    script: Option<ScriptScope>,
}

struct ScriptScope {
    range: Range<usize>,
    line: usize,
}

impl<'tree> ScopeIndex<'tree> {
    pub(super) fn new(root: Node<'tree>, source: &'tree [u8]) -> Self {
        let nodes = super::all_nodes(root);
        let contexts = syntax::context_parameters(&nodes, source);
        let recovered_types = syntax::recovered_types(root, source);
        let executable = nodes
            .iter()
            .filter_map(|node| {
                is_executable(*node, &recovered_types).then(|| (node.id(), node.byte_range()))
            })
            .collect();
        let mut script_nodes = (0..root.named_child_count())
            .filter_map(|index| root.named_child(index as u32))
            .filter(|child| is_top_level_executable(child.kind()))
            .filter(|child| !recovered_error(*child, &recovered_types));
        let script = script_nodes.next().map(|first| {
            let last = script_nodes.next_back().unwrap_or(first);
            ScriptScope {
                range: first.start_byte()..last.end_byte(),
                line: first.start_position().row + 1,
            }
        });
        Self {
            root,
            source,
            contexts,
            recovered_types,
            nodes,
            executable,
            script,
        }
    }

    pub(super) fn root(&self) -> Node<'tree> {
        self.root
    }

    pub(super) fn source(&self) -> &'tree [u8] {
        self.source
    }

    pub(super) fn contexts(&self) -> &[ContextParameters] {
        &self.contexts
    }

    pub(super) fn nodes(&self) -> &[Node<'tree>] {
        &self.nodes
    }

    pub(super) fn recovered_types(&self) -> &[syntax::RecoveredType] {
        &self.recovered_types
    }

    pub(super) fn metric_scope_range_for(&self, node: Node<'_>) -> Option<Range<usize>> {
        metric_scope_range_for(self, node)
    }
}

/// Return every syntactically executable Kotlin range. Bodyless declarations
/// stay out so expect/abstract/external APIs do not receive a synthetic
/// complexity baseline.
pub(in crate::languages::kotlin) fn capture(
    index: &ScopeIndex<'_>,
    query_functions: Vec<FunctionScope>,
) -> Vec<FunctionScope> {
    let source = index.source();
    let mut scopes = index
        .nodes()
        .iter()
        .copied()
        .filter_map(|node| {
            let range = index.executable.get(&node.id())?;
            Some(scope(
                scope_name(node, index, &query_functions),
                range.start,
                range.end,
                source,
            ))
        })
        .collect::<Vec<_>>();
    scopes.extend(index.script.iter().map(|script| {
        scope(
            format!("script@{}", script.line),
            script.range.start,
            script.range.end,
            source,
        )
    }));

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
    if let Some(identity) =
        super::ancestors(node).find_map(|candidate| index.executable.get(&candidate.id()))
    {
        return Some(identity.clone());
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

    index.script.as_ref().map(|script| script.range.clone())
}

pub(in crate::languages::kotlin) fn property_initializer_range(
    property: Node<'_>,
) -> Option<Range<usize>> {
    (property.kind() == "property_declaration"
        && has_property_initializer(property)
        && bound_function_value(property).is_none())
    .then(|| property.byte_range())
}

/// Classify executable nodes once, before query-dependent names are available.
fn is_executable(node: Node<'_>, recovered_types: &[syntax::RecoveredType]) -> bool {
    match node.kind() {
        "function_declaration" | "getter" | "setter" | "anonymous_function" => {
            has_named_child(node, "function_body")
        }
        "lambda_literal" | "anonymous_initializer" => true,
        "primary_constructor" => !has_bodyless_api_primary_constructor(node, recovered_types),
        "secondary_constructor" => has_direct_child(node, "{"),
        "property_declaration" => {
            has_property_initializer(node) && bound_function_value(node).is_none()
        }
        "destructuring_declaration" => has_direct_child(node, "="),
        "enum_entry" => has_named_child(node, "value_arguments"),
        _ => false,
    }
}

pub(super) fn has_named_child(node: Node<'_>, kind: &str) -> bool {
    direct_named_child(node, kind).is_some()
}
pub(super) fn has_direct_child(node: Node<'_>, kind: &str) -> bool {
    (0..node.child_count()).any(|index| node.child(index).is_some_and(|child| child.kind() == kind))
}
pub(super) fn has_property_initializer(node: Node<'_>) -> bool {
    has_named_child(node, "property_delegate") || has_direct_child(node, "=")
}
pub(super) fn bound_function_value(node: Node<'_>) -> Option<Node<'_>> {
    direct_named_child(node, "lambda_literal")
        .or_else(|| direct_named_child(node, "anonymous_function"))
        .or_else(|| {
            let annotated = direct_named_child(node, "annotated_lambda")?;
            direct_named_child(annotated, "lambda_literal")
        })
}
pub(super) fn normalized_source_identifier(range: &Range<usize>, source: &[u8]) -> String {
    String::from_utf8_lossy(&source[range.clone()])
        .trim_matches('`')
        .to_owned()
}
pub(super) fn direct_named_child<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    (0..node.named_child_count())
        .filter_map(|index| node.named_child(index as u32))
        .find(|child| child.kind() == kind)
}
pub(super) fn normalized_identifier(node: Node<'_>, source: &[u8]) -> String {
    let raw = String::from_utf8_lossy(&source[node.byte_range()]);
    raw.strip_prefix('`')
        .and_then(|inner| inner.strip_suffix('`'))
        .unwrap_or(&raw)
        .to_owned()
}
pub(super) fn is_top_level_executable(kind: &str) -> bool {
    !matches!(
        kind,
        "shebang_line"
            | "file_annotation"
            | "package_header"
            | "import_list"
            | "import_header"
            | "line_comment"
            | "multiline_comment"
            | "class_declaration"
            | "object_declaration"
            | "function_declaration"
            | "property_declaration"
            | "destructuring_declaration"
            | "getter"
            | "setter"
            | "type_alias"
            | "enum_entry"
    )
}

/// Return whether a node is inside a default argument before a callable boundary.
/// Each caller supplies the executable kinds that terminate its ownership search.
pub(super) fn is_default_argument(
    node: Node<'_>,
    source: &[u8],
    is_boundary: impl Fn(&str) -> bool,
) -> bool {
    for ancestor in super::ancestors(node).skip(1) {
        if is_boundary(ancestor.kind()) {
            return false;
        }
        if matches!(
            ancestor.kind(),
            "parameter" | "function_value_parameter" | "class_parameter"
        ) && source
            .get(ancestor.byte_range())
            .is_some_and(|parameter| parameter.contains(&b'='))
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
#[path = "../../../tests/unit/kotlin_scopes.rs"]
mod tests;
