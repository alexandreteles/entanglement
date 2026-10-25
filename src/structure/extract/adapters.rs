use tree_sitter::Node;

/// Grammar decisions used by syntax capture. Shared analysis does not read grammar names.
pub(super) fn declaration(kind: &str) -> Option<(&'static str, bool)> {
    match kind {
        "trait_item" | "interface_declaration" => Some(("interface", true)),
        "struct_item"
        | "enum_item"
        | "union_item"
        | "class_definition"
        | "class_declaration"
        | "abstract_class_declaration"
        | "object_declaration"
        | "trait_declaration" => Some(("type", true)),
        "type_spec" => Some(("type", true)),
        "type_item" | "associated_type" | "type_alias" | "type_alias_declaration" => {
            Some(("alias", true))
        }
        "function_item"
        | "function_definition"
        | "function_declaration"
        | "method_definition"
        | "method_declaration"
        | "method_signature"
        | "function_signature_item"
        | "method_elem"
        | "abstract_method_signature" => Some(("member", false)),
        "property_declaration"
        | "property_signature"
        | "field_definition"
        | "public_field_definition"
        | "field_declaration"
        | "const_item"
        | "const_declaration" => Some(("property", false)),
        "assignment" | "property_element" | "property_promotion_parameter" | "const_element" => {
            Some(("property", false))
        }
        _ => None,
    }
}

pub(super) fn classification(node: Node<'_>, owners: &[String]) -> Option<(&'static str, bool)> {
    if !owners.is_empty() && matches!(node.kind(), "associated_type" | "type_item") {
        return Some(("associated", false));
    }
    if node.kind() == "type_spec"
        && node
            .child_by_field_name("type")
            .is_some_and(|underlying| underlying.kind() == "interface_type")
    {
        return Some(("interface", true));
    }
    declaration(node.kind())
}

pub(super) fn name(node: Node<'_>) -> Option<Node<'_>> {
    if node.kind() == "assignment" {
        return node
            .child_by_field_name("left")
            .filter(|left| left.kind() == "identifier");
    }
    if node.kind() == "field_declaration" && node.child_by_field_name("name").is_none() {
        return node.child_by_field_name("type").and_then(type_leaf);
    }
    node.child_by_field_name("name").or_else(|| {
        let mut cursor = node.walk();
        node.named_children(&mut cursor).find(|child| {
            matches!(
                child.kind(),
                "simple_identifier"
                    | "identifier"
                    | "type_identifier"
                    | "property_element"
                    | "name"
            )
        })
    })
}

pub(super) fn additional_names(node: Node<'_>, source: &[u8]) -> Vec<String> {
    if node.kind() != "field_declaration" {
        return Vec::new();
    }
    let mut cursor = node.walk();
    node.children_by_field_name("name", &mut cursor)
        .skip(1)
        .map(|name| super::capture::text(name, source))
        .collect()
}

pub(super) fn relation(kind: &str) -> Option<super::super::model::RelationshipKind> {
    use super::super::model::RelationshipKind;
    match kind {
        "super_interfaces" | "implements_clause" => Some(RelationshipKind::Implement),
        "superclass"
        | "base_class_clause"
        | "extends_type_clause"
        | "extends_clause"
        | "delegation_specifiers"
        | "trait_bounds"
        | "class_heritage" => Some(RelationshipKind::Inherit),
        "use_declaration" | "trait_use_clause" => Some(RelationshipKind::Compose),
        _ => None,
    }
}

pub(super) fn type_node(kind: &str) -> bool {
    matches!(
        kind,
        "type_annotation"
            | "return_type"
            | "type_parameters"
            | "type_parameter_list"
            | "type_parameter_constraints"
            | "where_clause"
            | "where_predicate"
            | "generic_type"
            | "type_arguments"
            | "parameter_type"
            | "bounded_type"
            | "union_type"
            | "intersection_type"
            | "function_type"
            | "tuple_type"
    )
}

pub(super) fn limitations(language: &str) -> Vec<String> {
    let codes: &[&str] = if language.contains("python") {
        &["runtime_structure"]
    } else if language.contains("go") {
        &["structural_satisfaction", "generic_subst"]
    } else if language.contains("rust") {
        &[
            "generic_subst",
            "macro_output",
            "ext_bounds",
            "positional_members",
        ]
    } else if language.contains("php") {
        &["dynamic_dispatch", "version_properties"]
    } else if language.contains("kotlin") {
        &["build_variants", "generic_subst"]
    } else if language.contains("typescript") || language.contains("tsx") {
        &["conditional_types", "mapped_types", "structural_compat"]
    } else {
        &["dynamic_bindings"]
    };
    codes.iter().map(|code| (*code).to_owned()).collect()
}

pub(super) fn object_analysis(language: &str) -> bool {
    !["html", "css", "angular", "astro", "svelte"]
        .iter()
        .any(|name| language.contains(name))
}

pub(super) fn public(
    language: &str,
    node: Node<'_>,
    source: &[u8],
    name: &str,
    existing: Option<&crate::model::Definition>,
) -> bool {
    if language.contains("python") {
        return !name.starts_with('_');
    }
    if language.contains("go") {
        return name.chars().next().is_some_and(char::is_uppercase);
    }
    let prefix = node
        .child_by_field_name("body")
        .map_or(node.end_byte(), |body| body.start_byte());
    let header = String::from_utf8_lossy(&source[node.start_byte()..prefix]);
    if header
        .split_whitespace()
        .take(4)
        .any(|word| word == "private")
        || name.starts_with('#')
    {
        return false;
    }
    if language.contains("rust") {
        let owner =
            std::iter::successors(node.parent(), |ancestor| ancestor.parent()).find(|ancestor| {
                matches!(
                    ancestor.kind(),
                    "trait_item" | "function_item" | "impl_item" | "source_file"
                )
            });
        return owner.is_some_and(|owner| owner.kind() == "trait_item")
            || existing.is_some_and(|definition| definition.is_public)
            || header.starts_with("pub");
    }
    if language.contains("php") || language.contains("kotlin") {
        return existing
            .and_then(|definition| definition.declaration.as_ref())
            .is_none_or(|declaration| {
                matches!(
                    declaration.visibility,
                    crate::graph::SemanticVisibility::Public
                        | crate::graph::SemanticVisibility::Protected
                        | crate::graph::SemanticVisibility::Internal
                )
            });
    }
    true
}

pub(super) fn member_owner(node: Node<'_>, source: &[u8], owners: &[String]) -> Vec<String> {
    let nearest = std::iter::successors(node.parent(), |ancestor| ancestor.parent())
        .find(|ancestor| declaration(ancestor.kind()).is_some() || ancestor.kind() == "impl_item");
    if nearest.is_some_and(|ancestor| {
        declaration(ancestor.kind()).is_some_and(|(kind, _)| kind == "member")
    }) {
        return Vec::new();
    }
    node.child_by_field_name("receiver")
        .and_then(type_leaf)
        .map_or_else(
            || owners.to_vec(),
            |receiver_type| vec![super::capture::text(receiver_type, source)],
        )
}

pub(super) fn receiver(node: Node<'_>, source: &[u8]) -> Option<String> {
    if node.kind() == "field_declaration" && node.child_by_field_name("name").is_none() {
        let mut cursor = node.walk();
        let pointer = node
            .children(&mut cursor)
            .any(|child| child.kind() == "*" || child.kind() == "pointer_type");
        return Some(
            if pointer {
                "embed_pointer"
            } else {
                "embed_value"
            }
            .to_owned(),
        );
    }
    node.child_by_field_name("receiver").map(|receiver| {
        if super::capture::text(receiver, source).contains('*') {
            "pointer".to_owned()
        } else {
            "value".to_owned()
        }
    })
}

pub(super) fn impl_trait(node: Node<'_>, source: &[u8]) -> Option<String> {
    std::iter::successors(node.parent(), |ancestor| ancestor.parent())
        .find(|ancestor| ancestor.kind() == "impl_item")
        .and_then(|implementation| implementation.child_by_field_name("trait"))
        .map(|trait_node| super::capture::text(trait_node, source))
}

pub(super) fn required(
    node: Node<'_>,
    source: &[u8],
    kind: &str,
    owners: &[String],
    has_body: bool,
    existing: Option<&crate::model::Definition>,
) -> bool {
    let declaration = existing.and_then(|definition| definition.declaration.as_deref());
    if declaration.is_some_and(|declaration| declaration.modifiers.is_abstract) {
        return true;
    }
    let written = super::capture::text(node, source);
    if written
        .split_whitespace()
        .take(4)
        .any(|word| word == "abstract")
    {
        return true;
    }
    if optional(node) {
        return false;
    }
    match kind {
        "property" if node.kind() == "property_signature" => true,
        "member" if node.child_by_field_name("receiver").is_some() => false,
        "member" => !has_body,
        "associated" => {
            node.child_by_field_name("type").is_none()
                && node.child_by_field_name("value").is_none()
        }
        "property" if node.kind() == "const_item" => {
            !owners.is_empty() && node.child_by_field_name("value").is_none()
        }
        "property" => {
            declaration.is_some_and(|declaration| declaration.is_interface_member)
                && !has_body
                && !written.contains('=')
                && !written.contains("get(")
        }
        _ => false,
    }
}

pub(super) fn optional(node: Node<'_>) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor).any(|child| child.kind() == "?")
}

pub(super) fn is_type_position(node: Node<'_>) -> bool {
    type_node(node.kind())
        || node.parent().is_some_and(|parent| {
            ["type", "return_type", "result"]
                .into_iter()
                .filter_map(|field| parent.child_by_field_name(field))
                .any(|field| field.id() == node.id())
        })
}

pub(super) fn declared_relations(node: Node<'_>) -> Vec<super::SyntaxRelation> {
    use super::super::model::RelationshipKind;
    let mut relations = Vec::new();
    match node.kind() {
        "class_definition" => {
            if let Some(bases) = node.child_by_field_name("superclasses") {
                relations.push(super::SyntaxRelation {
                    start: bases.start_byte(),
                    end: bases.end_byte(),
                    kind: RelationshipKind::Inherit,
                    ordered: true,
                });
            }
        }
        "type_spec" => {
            if let Some(underlying) = node.child_by_field_name("type") {
                embedded(underlying, &mut relations);
            }
        }
        "type_alias_declaration" => {
            if let Some(value) = node.child_by_field_name("value") {
                alias_composition(value, &mut relations);
            }
        }
        "class_declaration" | "trait_declaration" => {
            if let Some(body) = super::capture::body_node(node) {
                let mut cursor = body.walk();
                relations.extend(
                    body.named_children(&mut cursor)
                        .filter(|child| {
                            matches!(child.kind(), "use_declaration" | "trait_use_clause")
                        })
                        .map(|child| super::SyntaxRelation {
                            start: child.start_byte(),
                            end: child.end_byte(),
                            kind: RelationshipKind::Compose,
                            ordered: false,
                        }),
                );
            }
        }
        _ => {}
    }
    relations
}

fn alias_composition(node: Node<'_>, relations: &mut Vec<super::SyntaxRelation>) {
    use super::super::model::RelationshipKind;
    match node.kind() {
        "type_identifier" | "nested_type_identifier" => relations.push(super::SyntaxRelation {
            start: node.start_byte(),
            end: node.end_byte(),
            kind: RelationshipKind::Compose,
            ordered: false,
        }),
        "generic_type" => {
            if let Some(name) = node.child_by_field_name("name") {
                alias_composition(name, relations);
            }
        }
        "intersection_type" | "parenthesized_type" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                alias_composition(child, relations);
            }
        }
        _ => {}
    }
}

fn embedded(node: Node<'_>, relations: &mut Vec<super::SyntaxRelation>) {
    use super::super::model::RelationshipKind;
    if node.kind() == "type_elem"
        || (node.kind() == "field_declaration" && node.child_by_field_name("name").is_none())
    {
        relations.push(super::SyntaxRelation {
            start: node.start_byte(),
            end: node.end_byte(),
            kind: RelationshipKind::Compose,
            ordered: false,
        });
        relations.extend(argument_relations(node));
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        embedded(child, relations);
    }
}

pub(super) fn argument_relations(node: Node<'_>) -> Vec<super::SyntaxRelation> {
    if node.kind() == "type_arguments" {
        return vec![super::SyntaxRelation {
            start: node.start_byte(),
            end: node.end_byte(),
            kind: super::super::model::RelationshipKind::Type,
            ordered: false,
        }];
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .flat_map(argument_relations)
        .collect()
}

fn type_leaf(node: Node<'_>) -> Option<Node<'_>> {
    if node.kind() == "type_identifier" {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor).find_map(type_leaf)
}

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
