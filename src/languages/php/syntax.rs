//! Small PHP syntax operations used by fact capture.

pub(super) use crate::languages::syntax::named_children as children;

use tree_sitter::Node;

use crate::graph::{PhpNamespace, SemanticVisibility};
use crate::model::SourceRange;

pub(super) fn nodes(root: Node<'_>) -> Vec<Node<'_>> {
    let mut nodes = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        nodes.push(node);
        pending.extend(children(node).rev());
    }
    nodes
}

pub(super) fn text<'a>(node: Node<'_>, source: &'a [u8]) -> &'a str {
    std::str::from_utf8(source.get(node.byte_range()).unwrap_or_default()).unwrap_or_default()
}

pub(super) fn range(node: Node<'_>) -> SourceRange {
    SourceRange {
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
    }
}

pub(super) fn ancestors(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    std::iter::successors(node.parent(), |node| node.parent())
}

pub(super) fn is_class(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "class_declaration"
            | "interface_declaration"
            | "trait_declaration"
            | "enum_declaration"
            | "anonymous_class"
    )
}

pub(super) fn is_callable(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "function_definition"
            | "method_declaration"
            | "anonymous_function"
            | "arrow_function"
            | "property_hook"
    )
}

pub(super) fn is_name(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "name" | "qualified_name" | "relative_name" | "relative_scope"
    )
}

pub(super) fn namespace<'a>(namespaces: &'a [PhpNamespace], node: Node<'_>) -> &'a PhpNamespace {
    namespaces
        .iter()
        .filter(|namespace| {
            namespace.range.start_byte <= node.start_byte()
                && node.end_byte() <= namespace.range.end_byte
        })
        .min_by_key(|namespace| namespace.range.end_byte - namespace.range.start_byte)
        .expect("the root namespace contains each node")
}

pub(super) fn class_name(
    node: Node<'_>,
    source: &[u8],
    namespaces: &[PhpNamespace],
) -> Option<String> {
    let class = ancestors(node)
        .find(|node| is_class(*node) || node.kind() == "function_definition")
        .filter(|node| is_class(*node))?;
    Some(namespace(namespaces, class).qualify(&declaration_name(class, source)))
}

pub(super) fn visibility(node: Node<'_>, source: &[u8]) -> SemanticVisibility {
    match children(node)
        .find(|child| {
            child.kind() == "visibility_modifier" && !text(*child, source).contains("set")
        })
        .map(|child| text(child, source))
    {
        Some("private") => SemanticVisibility::Private,
        Some("protected") => SemanticVisibility::Protected,
        _ => SemanticVisibility::Public,
    }
}

pub(super) fn modifier(node: Node<'_>, kind: &str) -> bool {
    children(node).any(|child| child.kind() == kind)
}

pub(super) fn conditional(node: Node<'_>) -> bool {
    ancestors(node)
        .take_while(|node| node.kind() != "anonymous_class")
        .any(|node| is_callable(node) || is_branch(node))
}

pub(super) fn name_parts(name: &str) -> Vec<String> {
    name.split('\\')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

pub(super) fn unwrap(mut node: Node<'_>) -> Node<'_> {
    while let ("parenthesized_expression", Some(inner)) = (node.kind(), node.named_child(0)) {
        node = inner;
    }
    node
}

pub(super) fn literal(node: Node<'_>, source: &[u8]) -> Option<String> {
    let value = text(node, source);
    if !matches!(node.kind(), "string" | "encapsed_string")
        || !matches!(value.as_bytes().first(), Some(b'\'' | b'"'))
    {
        return None;
    }
    let quote = *value.as_bytes().first()?;
    if value.as_bytes().last() != Some(&quote)
        || nodes(node)
            .iter()
            .any(|node| matches!(node.kind(), "variable_name" | "dynamic_variable_name"))
    {
        return None;
    }
    let inner = value.get(1..value.len().checked_sub(1)?)?;
    if quote == b'"' && inner.contains(['\\', '$']) {
        return None;
    }
    Some(inner.replace("\\\\", "\\").replace("\\'", "'"))
}

pub(super) fn first_class_callable(node: Node<'_>) -> bool {
    node.child_by_field_name("arguments")
        .is_some_and(|arguments| {
            children(arguments).any(|child| child.kind() == "variadic_placeholder")
        })
}

pub(super) fn declaration_name(node: Node<'_>, source: &[u8]) -> String {
    match node.child_by_field_name("name") {
        Some(name) => text(name, source).to_owned(),
        None if is_class(node) => format!("anonymous-class@{}", node.start_byte()),
        None => format!(
            "anonymous@{}:{}",
            node.start_position().row + 1,
            node.start_byte()
        ),
    }
}

pub(super) fn scope(node: Node<'_>) -> Node<'_> {
    ancestors(node)
        .find(|node| is_callable(*node))
        .unwrap_or_else(|| ancestors(node).last().unwrap_or(node))
}

pub(super) fn is_branch(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "if_statement"
            | "else_if_clause"
            | "else_clause"
            | "switch_statement"
            | "for_statement"
            | "foreach_statement"
            | "while_statement"
            | "do_statement"
            | "catch_clause"
    )
}
