use std::ops::Range;

use tree_sitter::Node;

use crate::metrics::cyclomatic::FunctionScope;

pub(super) fn query_name(functions: &[FunctionScope], node: Node<'_>) -> Option<String> {
    functions
        .iter()
        .find(|function| {
            function.range.start == node.start_byte() && function.range.end == node.end_byte()
        })
        .map(|function| function.name.clone())
}
pub(super) fn scope(name: String, start: usize, end: usize, source: &[u8]) -> FunctionScope {
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
pub(super) fn has_named_child(node: Node<'_>, kind: &str) -> bool {
    (0..node.named_child_count()).any(|index| {
        node.named_child(index as u32)
            .is_some_and(|child| child.kind() == kind)
    })
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
