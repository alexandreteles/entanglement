use std::ops::Range;

use tree_sitter::Node;

#[derive(Clone, Copy)]
pub(super) enum LocalKind {
    Binding,
    Parameter,
    Catch,
    Loop,
}

pub(crate) fn binding_names(node: Node<'_>, source: &[u8]) -> Vec<(String, Range<usize>)> {
    let mut names = Vec::new();
    let mut pending = vec![node];
    while let Some(current) = pending.pop() {
        match current.kind() {
            "identifier" | "shorthand_property_identifier_pattern" => {
                names.push((text(current, source), current.byte_range()));
            }
            "member_expression" | "subscript_expression" | "type_annotation" => {}
            "required_parameter" | "optional_parameter" => {
                if let Some(pattern) = parameter_pattern(current) {
                    pending.push(pattern);
                }
            }
            "assignment_pattern" | "object_assignment_pattern" => {
                if let Some(left) = current.child_by_field_name("left") {
                    pending.push(left);
                }
            }
            "pair_pattern" => {
                if let Some(value) = current.child_by_field_name("value") {
                    pending.push(value);
                }
            }
            "property_identifier" | "type_identifier" | "this" => {}
            _ => {
                let mut cursor = current.walk();
                pending.extend(current.named_children(&mut cursor));
            }
        }
    }
    names
}

pub(super) fn parameter_pattern(node: Node<'_>) -> Option<Node<'_>> {
    match node.kind() {
        "required_parameter" | "optional_parameter" => node
            .child_by_field_name("pattern")
            .or_else(|| node.child_by_field_name("name")),
        "assignment_pattern" => node.child_by_field_name("left"),
        _ => Some(node),
    }
}

pub(super) fn local_scope(
    node: Node<'_>,
    kind: LocalKind,
    root: Range<usize>,
    source: &[u8],
) -> (usize, usize) {
    let scope = match kind {
        LocalKind::Parameter => nearest(node, is_function).unwrap_or(node),
        LocalKind::Catch => nearest_kind(node, "catch_clause").unwrap_or(node),
        LocalKind::Loop => nearest_loop(node).unwrap_or(node),
        LocalKind::Binding => binding_scope(node, source).unwrap_or(node),
    };
    let range = scope.byte_range();
    if range.start == range.end {
        (root.start, root.end)
    } else {
        (range.start, range.end)
    }
}

fn binding_scope<'tree>(node: Node<'tree>, source: &[u8]) -> Option<Node<'tree>> {
    let declaration = nearest_kind(node, "variable_declaration")
        .or_else(|| nearest_kind(node, "lexical_declaration"))?;
    let kind = declaration
        .child_by_field_name("kind")
        .map(|item| String::from_utf8_lossy(&source[item.byte_range()]).into_owned());
    if kind.as_deref() == Some("var") {
        return nearest(declaration, is_function).or_else(|| nearest_kind(declaration, "program"));
    }
    nearest_binding_scope(declaration)
}

fn nearest_binding_scope(node: Node<'_>) -> Option<Node<'_>> {
    nearest_from(node, false, |item| {
        is_scope(item) || matches!(item.kind(), "for_statement" | "for_in_statement")
    })
}

pub(super) fn scope_range(node: Node<'_>, root: Range<usize>, skip_self: bool) -> (usize, usize) {
    nearest_from(node, skip_self, is_scope)
        .map(|scope| {
            let range = scope.byte_range();
            (range.start, range.end)
        })
        .unwrap_or((root.start, root.end))
}

pub(super) fn nearest_loop(node: Node<'_>) -> Option<Node<'_>> {
    nearest(node, |item| {
        matches!(item.kind(), "for_statement" | "for_in_statement")
    })
}

pub(super) fn nearest_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    nearest(node, |item| item.kind() == kind)
}

pub(super) fn nearest(node: Node<'_>, predicate: impl Fn(Node<'_>) -> bool) -> Option<Node<'_>> {
    nearest_from(node, false, predicate)
}

pub(super) fn nearest_from(
    node: Node<'_>,
    skip_self: bool,
    predicate: impl Fn(Node<'_>) -> bool,
) -> Option<Node<'_>> {
    let mut current = if skip_self { node.parent() } else { Some(node) };
    while let Some(item) = current {
        if predicate(item) {
            return Some(item);
        }
        current = item.parent();
    }
    None
}

fn is_scope(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "statement_block" | "program" | "switch_body" | "catch_clause" | "class_body"
    )
}

pub(super) fn is_function(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "function_declaration"
            | "function_expression"
            | "generator_function_declaration"
            | "generator_function"
            | "arrow_function"
            | "method_definition"
    )
}

pub(super) fn enclosing_function(node: Node<'_>) -> Option<usize> {
    nearest_from(node, true, is_function).map(|function| function.start_byte())
}

pub(super) fn contains(outer: &Range<usize>, inner: &Range<usize>) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

pub(super) fn text(node: Node<'_>, source: &[u8]) -> String {
    String::from_utf8_lossy(&source[node.byte_range()]).into_owned()
}
