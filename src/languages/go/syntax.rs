use std::ops::Range;

use tree_sitter::Node;

use crate::model::{DefinitionKind, Import, ModulePath, ReferenceKind};

/// Go exports a package-level name when it starts with an upper-case letter.
pub(super) fn is_exported(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

pub(super) fn type_kind(spec: Node<'_>) -> DefinitionKind {
    match spec.child_by_field_name("type").map(|node| node.kind()) {
        Some("struct_type") => DefinitionKind::Struct,
        Some("interface_type") => DefinitionKind::Trait,
        _ => DefinitionKind::Other,
    }
}

pub(super) fn import(spec: Node<'_>, source: &[u8]) -> Import {
    let path = spec
        .child_by_field_name("path")
        .and_then(|node| super::literals::unquote(&text(node, source)));
    Import {
        path: path
            .as_deref()
            .unwrap_or_default()
            .split('/')
            .map(str::to_owned)
            .collect(),
        alias: spec
            .child_by_field_name("name")
            .map(|node| text(node, source)),
        source: path,
        imported_name: None,
        namespace: true,
        start_byte: spec.start_byte(),
        end_byte: spec.end_byte(),
        module: ModulePath::default(),
        scope_start: 0,
        scope_end: 0,
        is_public: false,
        context_id: 0,
    }
}

/// Only syntactically direct calls can become call-graph edges.
pub(super) fn reference_target<'a>(node: Node<'a>, kind: &ReferenceKind) -> Option<Node<'a>> {
    if *kind != ReferenceKind::Call {
        return Some(node);
    }
    let node = call_base(node)?;
    let direct = match node.kind() {
        "identifier" | "type_identifier" | "qualified_type" => true,
        "selector_expression" => node.child_by_field_name("operand")?.kind() == "identifier",
        _ => false,
    };
    direct.then_some(node)
}

fn call_base(mut node: Node<'_>) -> Option<Node<'_>> {
    loop {
        node = match node.kind() {
            "parenthesized_expression" | "parenthesized_type" => node.named_child(0)?,
            "generic_type" | "type_instantiation_expression" => node.child_by_field_name("type")?,
            _ => break,
        };
    }
    Some(node)
}

pub(super) fn reference_path(node: Node<'_>, source: &[u8]) -> Vec<String> {
    let fields = match node.kind() {
        "selector_expression" => ["operand", "field"],
        "qualified_type" => ["package", "name"],
        _ => return vec![text(node, source)],
    };
    fields
        .into_iter()
        .filter_map(|field| node.child_by_field_name(field))
        .map(|child| text(child, source))
        .collect()
}

pub(super) fn text(node: Node<'_>, source: &[u8]) -> String {
    String::from_utf8_lossy(&source[node.byte_range()]).into_owned()
}

pub(super) fn contains(outer: &Range<usize>, inner: &Range<usize>) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}
