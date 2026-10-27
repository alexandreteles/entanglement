use tree_sitter::Node;

pub(super) use super::adapter_members::{constraints, contract_fields, member_kind};
pub(super) use super::adapter_relations::{
    argument_relations, declared_relations, is_type_position, relation,
};

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

use super::adapter_ownership::type_leaf;
pub(super) use super::adapter_ownership::{impl_trait, member_owner, receiver};

pub(super) use super::adapter_visibility::{optional, public, required};
