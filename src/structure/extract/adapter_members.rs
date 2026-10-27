use tree_sitter::Node;

pub(super) fn member_kind(node: Node<'_>, source: &[u8]) -> Option<String> {
    if !matches!(
        node.kind(),
        "method_definition" | "method_signature" | "abstract_method_signature"
    ) {
        return None;
    }
    let name = node.child_by_field_name("name")?;
    if name.kind() == "computed_property_name" {
        return None;
    }
    let mut cursor = node.walk();
    let modifiers = node
        .children(&mut cursor)
        .filter(|child| !child.is_named())
        .map(|child| child.kind())
        .collect::<Vec<_>>();
    let is_static = modifiers.contains(&"static");
    if !is_static && super::capture::text(name, source) == "constructor" {
        return Some("constructor".to_owned());
    }
    let accessor = modifiers
        .into_iter()
        .find(|kind| matches!(*kind, "get" | "set"));
    Some(
        match (is_static, accessor) {
            (false, None) => "instance",
            (true, None) => "static",
            (false, Some("get")) => "get",
            (false, _) => "set",
            (true, Some("get")) => "static_get",
            (true, _) => "static_set",
        }
        .to_owned(),
    )
}

pub(super) fn constraints(
    node: Node<'_>,
    source: &[u8],
) -> Vec<crate::structure::types::TypeExpression> {
    if node.kind() != "type_spec" {
        return Vec::new();
    }
    let Some(underlying) = node
        .child_by_field_name("type")
        .filter(|underlying| underlying.kind() == "interface_type")
    else {
        return Vec::new();
    };
    let mut cursor = underlying.walk();
    underlying
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "type_elem")
        .map(|child| crate::structure::types::TypeExpression::from_node(child, source))
        .collect()
}

pub(super) fn contract_fields(node: Node<'_>) -> Vec<Node<'_>> {
    match node.kind() {
        "struct_item" => node
            .child_by_field_name("body")
            .filter(|body| body.kind() == "ordered_field_declaration_list")
            .map_or_else(Vec::new, |body| positional_fields(body, false)),
        "enum_item" => {
            let Some(body) = node.child_by_field_name("body") else {
                return Vec::new();
            };
            let mut cursor = body.walk();
            body.named_children(&mut cursor)
                .filter_map(|variant| variant.child_by_field_name("body"))
                .flat_map(enum_fields)
                .collect()
        }
        _ => Vec::new(),
    }
}

fn enum_fields(body: Node<'_>) -> Vec<Node<'_>> {
    if body.kind() == "ordered_field_declaration_list" {
        return positional_fields(body, true);
    }
    let mut cursor = body.walk();
    body.named_children(&mut cursor)
        .filter_map(|field| field.child_by_field_name("type"))
        .collect()
}

fn positional_fields(body: Node<'_>, inherited_visibility: bool) -> Vec<Node<'_>> {
    let mut cursor = body.walk();
    let field_ids = body
        .children_by_field_name("type", &mut cursor)
        .map(|field| field.id())
        .collect::<std::collections::BTreeSet<_>>();
    let mut public = inherited_visibility;
    let mut fields = Vec::new();
    for child in body.named_children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            public = true;
        }
        if field_ids.contains(&child.id()) {
            if public {
                fields.push(child);
            }
            public = inherited_visibility;
        }
    }
    fields
}
