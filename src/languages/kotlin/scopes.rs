use std::ops::Range;

use tree_sitter::Node;

use crate::metrics::cyclomatic::FunctionScope;

use super::syntax::{self, ContextParameters};

/// Return every syntactically executable Kotlin range. Bodyless declarations
/// stay out so expect/abstract/external APIs do not receive a synthetic
/// complexity baseline.
pub(super) fn capture(
    root: Node<'_>,
    source: &[u8],
    contexts: &[ContextParameters],
    query_functions: Vec<FunctionScope>,
) -> Vec<FunctionScope> {
    let mut scopes = Vec::new();
    let mut pending = vec![root];
    let mut script_nodes = Vec::new();
    let recovered_types = syntax::recovered_types(root, source);

    while let Some(node) = pending.pop() {
        let name = executable_scope_name(node, source, &query_functions, &recovered_types);
        if let Some(name) = name {
            let start = syntax::context_start(node, contexts);
            scopes.push(scope(name, start, node.end_byte(), source));
        }

        if node.kind() == "source_file" {
            for index in 0..node.named_child_count() {
                if let Some(child) = node.named_child(index as u32)
                    && is_top_level_executable(child.kind())
                    && !recovered_error(child, &recovered_types)
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
    root: Node<'_>,
    node: Node<'_>,
    source: &[u8],
    contexts: &[ContextParameters],
) -> Option<Range<usize>> {
    let recovered_types = syntax::recovered_types(root, source);
    let mut current = Some(node);
    while let Some(candidate) = current {
        if executable_scope_name(candidate, source, &[], &recovered_types).is_some() {
            return Some(syntax::context_start(candidate, contexts)..candidate.end_byte());
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
        .filter(|child| !recovered_error(*child, &recovered_types))
        .collect::<Vec<_>>();
    let first = script_nodes.first()?;
    let last = script_nodes.last()?;
    Some(first.start_byte()..last.end_byte())
}

pub(super) fn property_initializer_range(
    property: Node<'_>,
    contexts: &[ContextParameters],
) -> Option<Range<usize>> {
    (property.kind() == "property_declaration"
        && has_property_initializer(property)
        && bound_function_value(property).is_none())
    .then(|| syntax::context_start(property, contexts)..property.end_byte())
}

fn executable_scope_name(
    node: Node<'_>,
    source: &[u8],
    query_functions: &[FunctionScope],
    recovered_types: &[syntax::RecoveredType],
) -> Option<String> {
    match node.kind() {
        "function_declaration" if has_named_child(node, "function_body") => {
            Some(declaration_name(node, source, "function"))
        }
        "lambda_literal" => Some(
            bound_lambda_name(node, source)
                .or_else(|| query_name(query_functions, node))
                .unwrap_or_else(|| format!("anonymous@{}", node.start_position().row + 1)),
        ),
        "anonymous_function" if has_named_child(node, "function_body") => Some(
            bound_lambda_name(node, source)
                .or_else(|| query_name(query_functions, node))
                .unwrap_or_else(|| format!("anonymous@{}", node.start_position().row + 1)),
        ),
        "property_declaration"
            if has_property_initializer(node) && bound_function_value(node).is_none() =>
        {
            Some(property_name(node, source))
        }
        "destructuring_declaration" if has_direct_child(node, "=") => {
            Some(format!("destructuring@{}", node.start_position().row + 1))
        }
        "primary_constructor" if !has_bodyless_api_primary_constructor(node, recovered_types) => {
            Some(primary_constructor_name(node, source, recovered_types))
        }
        "secondary_constructor" if has_direct_child(node, "{") => Some(format!(
            "{}.constructor",
            enclosing_type_name(node, source, recovered_types)
        )),
        "anonymous_initializer" => Some("init".to_owned()),
        "getter" if has_named_child(node, "function_body") => Some("get".to_owned()),
        "setter" if has_named_child(node, "function_body") => Some("set".to_owned()),
        "enum_entry" if has_named_child(node, "value_arguments") => {
            Some(declaration_name(node, source, "enum_entry"))
        }
        _ => None,
    }
}

fn query_name(functions: &[FunctionScope], node: Node<'_>) -> Option<String> {
    functions
        .iter()
        .find(|function| {
            function.range.start == node.start_byte() && function.range.end == node.end_byte()
        })
        .map(|function| function.name.clone())
}

fn scope(name: String, start: usize, end: usize, source: &[u8]) -> FunctionScope {
    FunctionScope {
        name,
        range: start..end,
        line: source[..start.min(source.len())]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count()
            + 1,
    }
}

fn has_named_child(node: Node<'_>, kind: &str) -> bool {
    (0..node.named_child_count()).any(|index| {
        node.named_child(index as u32)
            .is_some_and(|child| child.kind() == kind)
    })
}

fn has_direct_child(node: Node<'_>, kind: &str) -> bool {
    (0..node.child_count()).any(|index| node.child(index).is_some_and(|child| child.kind() == kind))
}

fn has_property_initializer(node: Node<'_>) -> bool {
    has_named_child(node, "property_delegate") || has_direct_child(node, "=")
}

fn bound_function_value(node: Node<'_>) -> Option<Node<'_>> {
    direct_named_child(node, "lambda_literal")
        .or_else(|| direct_named_child(node, "anonymous_function"))
        .or_else(|| {
            let annotated = direct_named_child(node, "annotated_lambda")?;
            direct_named_child(annotated, "lambda_literal")
        })
}

fn bound_lambda_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    let mut parent = node.parent()?;
    if parent.kind() == "annotated_lambda" {
        parent = parent.parent()?;
    }
    (parent.kind() == "property_declaration"
        && bound_function_value(parent).is_some_and(|value| value.id() == node.id()))
    .then(|| property_name(parent, source))
}

fn declaration_name(node: Node<'_>, source: &[u8], fallback: &str) -> String {
    let candidate = match node.kind() {
        "enum_entry" => direct_named_child(node, "simple_identifier"),
        "function_declaration" => direct_named_child(node, "simple_identifier"),
        _ => node.child_by_field_name("name"),
    };
    candidate
        .map(|name| normalized_identifier(name, source))
        .unwrap_or_else(|| format!("{fallback}@{}", node.start_position().row + 1))
}

fn property_name(node: Node<'_>, source: &[u8]) -> String {
    direct_named_child(node, "variable_declaration")
        .and_then(|declaration| direct_named_child(declaration, "simple_identifier"))
        .map(|name| normalized_identifier(name, source))
        .unwrap_or_else(|| format!("property@{}", node.start_position().row + 1))
}

fn primary_constructor_name(
    node: Node<'_>,
    source: &[u8],
    recovered_types: &[syntax::RecoveredType],
) -> String {
    node.parent()
        .filter(|parent| parent.kind() == "class_declaration")
        .and_then(|class| direct_named_child(class, "type_identifier"))
        .map(|name| normalized_identifier(name, source))
        .or_else(|| {
            syntax::recovered_type_for(node, recovered_types)
                .and_then(|recovered| recovered.name_range.as_ref())
                .map(|range| normalized_source_identifier(range, source))
        })
        .unwrap_or_else(|| format!("constructor@{}", node.start_position().row + 1))
}

fn has_bodyless_api_primary_constructor(
    node: Node<'_>,
    recovered_types: &[syntax::RecoveredType],
) -> bool {
    if node
        .parent()
        .filter(|parent| matches!(parent.kind(), "class_declaration" | "object_declaration"))
        .and_then(|declaration| direct_named_child(declaration, "modifiers"))
        .is_some_and(|modifiers| {
            has_descendant_kind(modifiers, "expect") || has_descendant_kind(modifiers, "external")
        })
    {
        return true;
    }
    let Some(recovered) = syntax::recovered_type_for(node, recovered_types) else {
        return false;
    };
    std::iter::successors(node.parent(), |ancestor| ancestor.parent())
        .find(|ancestor| {
            ancestor.kind() == "ERROR" && ancestor.byte_range() == recovered.error_range
        })
        .and_then(|error| {
            (0..error.child_count())
                .filter_map(|index| error.child(index))
                .find(|child| {
                    child.kind() == "modifiers" && child.end_byte() <= recovered.keyword_range.start
                })
        })
        .is_some_and(|modifiers| {
            has_descendant_kind(modifiers, "expect") || has_descendant_kind(modifiers, "external")
        })
}

fn has_descendant_kind(node: Node<'_>, kind: &str) -> bool {
    let mut pending = vec![node];
    while let Some(candidate) = pending.pop() {
        if candidate.kind() == kind {
            return true;
        }
        for index in (0..candidate.child_count()).rev() {
            if let Some(child) = candidate.child(index) {
                pending.push(child);
            }
        }
    }
    false
}

fn enclosing_type_name(
    node: Node<'_>,
    source: &[u8],
    recovered_types: &[syntax::RecoveredType],
) -> String {
    let mut current = node.parent();
    while let Some(parent) = current {
        if matches!(parent.kind(), "class_declaration" | "object_declaration") {
            return direct_named_child(parent, "type_identifier")
                .map(|name| normalized_identifier(name, source))
                .unwrap_or_else(|| "anonymous".to_owned());
        }
        current = parent.parent();
    }
    syntax::recovered_type_for(node, recovered_types)
        .and_then(|recovered| recovered.name_range.as_ref())
        .map(|range| normalized_source_identifier(range, source))
        .unwrap_or_else(|| "constructor".to_owned())
}

fn recovered_error(node: Node<'_>, recovered_types: &[syntax::RecoveredType]) -> bool {
    recovered_types
        .iter()
        .any(|recovered| recovered.error_range == node.byte_range())
}

fn normalized_source_identifier(range: &Range<usize>, source: &[u8]) -> String {
    String::from_utf8_lossy(&source[range.clone()])
        .trim_matches('`')
        .to_owned()
}

fn direct_named_child<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    (0..node.named_child_count())
        .filter_map(|index| node.named_child(index as u32))
        .find(|child| child.kind() == kind)
}

fn normalized_identifier(node: Node<'_>, source: &[u8]) -> String {
    let raw = String::from_utf8_lossy(&source[node.byte_range()]);
    raw.strip_prefix('`')
        .and_then(|inner| inner.strip_suffix('`'))
        .unwrap_or(&raw)
        .to_owned()
}

fn is_top_level_executable(kind: &str) -> bool {
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
