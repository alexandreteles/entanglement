use tree_sitter::Node;

use crate::languages::{CapturedTree, InjectionRequest};

use super::{children, text};

/// Analyze static Angular templates without changing source byte positions.
/// Escaped strings and template substitutions require source maps and are skipped.
pub(in crate::languages::web) fn inline_templates(
    root: Node<'_>,
    source: &[u8],
    captured: &mut CapturedTree,
) {
    if !captured
        .facts
        .imports
        .iter()
        .any(|import| import.source.as_deref() == Some("@angular/core"))
    {
        return;
    }
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if node.kind() == "call_expression"
            && component_call(node, source, captured)
            && let Some(metadata) = node
                .child_by_field_name("arguments")
                .and_then(|arguments| children(arguments).next())
                .filter(|metadata| metadata.kind() == "object")
        {
            captured
                .injections
                .extend(children(metadata).filter_map(|pair| template(pair, source)));
        }
        pending.extend(children(node));
    }
}

fn component_call(node: Node<'_>, source: &[u8], captured: &CapturedTree) -> bool {
    let Some(function) = node.child_by_field_name("function") else {
        return false;
    };
    let (binding, namespace) = match function.kind() {
        "identifier" => (text(function, source), false),
        "member_expression" => {
            if function
                .child_by_field_name("property")
                .map(|property| text(property, source))
                != Some("Component")
            {
                return false;
            }
            let Some(object) = function
                .child_by_field_name("object")
                .filter(|node| node.kind() == "identifier")
            else {
                return false;
            };
            (text(object, source), true)
        }
        _ => return false,
    };
    captured.facts.imports.iter().any(|import| {
        import.source.as_deref() == Some("@angular/core")
            && import.alias.as_deref() == Some(binding)
            && import.namespace == namespace
            && (namespace || import.imported_name.as_deref() == Some("Component"))
    }) && !captured.facts.locals.iter().any(|local| {
        local.name == binding
            && local.scope_start <= node.start_byte()
            && node.end_byte() <= local.scope_end
    }) && !captured.facts.definitions.iter().any(|definition| {
        definition.name == binding
            && definition.scope_start <= node.start_byte()
            && node.end_byte() <= definition.scope_end
    })
}

fn template(pair: Node<'_>, source: &[u8]) -> Option<InjectionRequest> {
    if pair.kind() != "pair" {
        return None;
    }
    let key = pair.child_by_field_name("key")?;
    if text(key, source).trim_matches(['\'', '"']) != "template" {
        return None;
    }
    let value = pair.child_by_field_name("value")?;
    if !matches!(value.kind(), "string" | "template_string")
        || children(value)
            .any(|node| matches!(node.kind(), "escape_sequence" | "template_substitution"))
        || text(value, source).contains('\\')
    {
        return None;
    }
    let range = value.start_byte() + 1..value.end_byte().checked_sub(1)?;
    if range.is_empty() {
        return None;
    }
    let mut start = value.start_position();
    start.column += 1;
    let mut end = value.end_position();
    end.column = end.column.checked_sub(1)?;
    Some(InjectionRequest {
        language: "angular".into(),
        range: range.clone(),
        guest_ranges: vec![tree_sitter::Range {
            start_byte: range.start,
            end_byte: range.end,
            start_point: start,
            end_point: end,
        }],
        priority: 3,
        inherit_scope: false,
        inherit_metrics: false,
        inherit_context: false,
        share_bindings: false,
        publish_exports: false,
        registered_only: false,
        serialized_bindings: Vec::new(),
    })
}
