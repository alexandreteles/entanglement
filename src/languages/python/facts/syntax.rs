use std::ops::Range;

use tree_sitter::Node;

use crate::model::ModulePath;

pub(super) fn text(node: Node<'_>, source: &[u8]) -> String {
    String::from_utf8_lossy(&source[node.byte_range()]).into_owned()
}

pub(super) fn children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

pub(super) fn nodes(root: Node<'_>) -> Vec<Node<'_>> {
    let mut result = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        result.push(node);
        pending.extend(children(node));
    }
    result
}

pub(super) fn range(node: Node<'_>) -> Range<usize> {
    node.start_byte()..node.end_byte()
}

pub(super) fn field<'tree>(node: Node<'tree>, name: &str) -> Option<Node<'tree>> {
    node.child_by_field_name(name)
}

pub(super) fn module_path() -> ModulePath {
    ModulePath::default()
}

pub(super) fn ancestors<'tree>(node: Node<'tree>) -> impl Iterator<Item = Node<'tree>> {
    std::iter::successors(Some(node), |item| item.parent())
}

pub(super) fn body(node: Node<'_>) -> Node<'_> {
    field(node, "body").unwrap_or(node)
}

pub(super) fn is_top_level(node: Node<'_>) -> bool {
    let mut current = node.parent();
    while let Some(item) = current {
        if matches!(item.kind(), "function_definition" | "class_definition") {
            return false;
        }
        current = item.parent();
    }
    true
}

pub(super) fn target_names(node: Node<'_>, source: &[u8]) -> Vec<(String, Range<usize>)> {
    let mut result = Vec::new();
    let mut pending = vec![node];
    while let Some(item) = pending.pop() {
        match item.kind() {
            "identifier" | "as_pattern_target" => {
                result.push((text(item, source), range(item)));
            }
            "attribute" | "subscript" | "type" | "keyword_identifier" => {}
            "tuple_pattern"
            | "list_pattern"
            | "pattern_list"
            | "pattern"
            | "list_splat_pattern"
            | "dictionary_splat_pattern"
            | "splat_pattern" => {
                pending.extend(children(item));
            }
            _ => {}
        }
    }
    result
}

pub(super) fn direct_function(node: Node<'_>) -> Option<Node<'_>> {
    let value = field(node, "right")?;
    (value.kind() == "lambda").then_some(value)
}
