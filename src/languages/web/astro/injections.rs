use std::ops::Range;

use tree_sitter::Node;

use crate::languages::InjectionRequest;
use crate::model::LocalBinding;

use super::scopes::{ancestor, named_children};

pub(super) fn configure(
    root: Node<'_>,
    source: &[u8],
    ignored_before: usize,
    requests: &mut [InjectionRequest],
) {
    for request in requests {
        if request.range.start < ignored_before {
            continue;
        }
        let Some(body) = raw_body(root, &request.range) else {
            continue;
        };
        let Some(element) = ancestor(body, |node| node.kind() == "jsx_element") else {
            continue;
        };
        let Some(opening) = element.child_by_field_name("open_tag") else {
            continue;
        };
        let Some(tag) = tag_name(opening, source) else {
            continue;
        };
        if !matches!(tag.as_str(), "script" | "style") {
            continue;
        }
        request.inherit_scope = false;
        request.inherit_metrics = false;
        request.inherit_context = false;
        request.share_bindings = false;
        request.publish_exports = false;
        if tag == "script" {
            request.serialized_bindings = define_vars(opening, source, &request.range);
        }
    }
}

fn raw_body<'tree>(root: Node<'tree>, range: &Range<usize>) -> Option<Node<'tree>> {
    root.descendant_for_byte_range(range.start, range.end)
        .filter(|node| {
            node.kind() == "raw_text"
                && node.start_byte() == range.start
                && node.end_byte() == range.end
        })
}

fn tag_name(opening: Node<'_>, source: &[u8]) -> Option<String> {
    opening
        .child_by_field_name("name")
        .map(|node| text(node, source))
}

fn define_vars(opening: Node<'_>, source: &[u8], body: &Range<usize>) -> Vec<LocalBinding> {
    named_children(opening)
        .filter(|node| node.kind() == "jsx_attribute")
        .filter_map(|attribute| {
            let name = attribute.child_by_field_name("name")?;
            (text(name, source) == "define:vars")
                .then(|| attribute.child_by_field_name("value"))
                .flatten()
        })
        .flat_map(|value| serialized_names(value, source))
        .map(|(name, range)| LocalBinding {
            name,
            start_byte: range.start,
            end_byte: range.end,
            scope_start: body.start,
            scope_end: body.end,
            context_id: 0,
        })
        .collect()
}

fn serialized_names(value: Node<'_>, source: &[u8]) -> Vec<(String, Range<usize>)> {
    let Some(object) = object_value(value) else {
        return Vec::new();
    };
    named_children(object)
        .filter_map(|property| match property.kind() {
            "pair" => property
                .child_by_field_name("key")
                .and_then(|key| static_key(key, source)),
            "shorthand_property_identifier" | "identifier" => {
                let name = text(property, source);
                valid_identifier(&name).then(|| (name, property.byte_range()))
            }
            _ => None,
        })
        .collect()
}

fn object_value(mut node: Node<'_>) -> Option<Node<'_>> {
    loop {
        match node.kind() {
            "jsx_expression" | "parenthesized_expression" => {
                node = named_children(node).find(|child| child.kind() != "comment")?;
            }
            "object" => return Some(node),
            _ => return None,
        }
    }
}

fn static_key(node: Node<'_>, source: &[u8]) -> Option<(String, Range<usize>)> {
    let raw = text(node, source);
    let name = match node.kind() {
        "identifier" | "property_identifier" => raw,
        "string" => unquote_static_string(&raw)?.to_owned(),
        _ => return None,
    };
    valid_identifier(&name).then(|| (name, node.byte_range()))
}

fn unquote_static_string(value: &str) -> Option<&str> {
    let quote = value.chars().next()?;
    if !matches!(quote, '\'' | '"') || !value.ends_with(quote) {
        return None;
    }
    let inner = &value[quote.len_utf8()..value.len() - quote.len_utf8()];
    (!inner.contains('\\')).then_some(inner)
}

fn valid_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (first == '_' || first == '$' || first.is_alphabetic())
        && characters
            .all(|character| character == '_' || character == '$' || character.is_alphanumeric())
}

fn text(node: Node<'_>, source: &[u8]) -> String {
    String::from_utf8_lossy(&source[node.byte_range()]).into_owned()
}
