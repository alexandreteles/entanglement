use super::expressions::simple;
use super::{Decl, Parameter};
use crate::languages::syntax::{named_children, text};
use crate::model::DefinitionKind;
use crate::structure::types::TypeExpression;
use tree_sitter::Node;

pub(super) fn read(node: Node<'_>, source: &[u8], context: &super::walk::Context) -> Option<Decl> {
    let named = |field| node.child_by_field_name(field);
    let parent = node.parent();
    let variable = parent.and_then(|n| n.parent()).map(|n| n.kind());
    let recovered_union = partial_union(node, source);
    if node.kind() == "variable_declarator"
        && parent
            .and_then(|n| n.parent())
            .is_some_and(|n| partial_union(n, source))
    {
        return None;
    }
    let (kind, category, is_type) = if recovered_union {
        (DefinitionKind::Union, "union", true)
    } else {
        match node.kind() {
            "class_declaration" => (DefinitionKind::Struct, "class", true),
            "struct_declaration" => (DefinitionKind::Struct, "struct", true),
            "record_declaration" => (DefinitionKind::Struct, "record", true),
            "interface_declaration" => (DefinitionKind::Trait, "interface", true),
            "enum_declaration" => (DefinitionKind::Enum, "enum", true),
            "union_declaration" => (DefinitionKind::Union, "union", true),
            "delegate_declaration" => (DefinitionKind::TypeAlias, "delegate", true),
            "method_declaration" => (DefinitionKind::Method, "method", false),
            "local_function_statement" => (DefinitionKind::Function, "local", false),
            "constructor_declaration" => (DefinitionKind::Method, "constructor", false),
            "destructor_declaration" => (DefinitionKind::Method, "finalizer", false),
            "operator_declaration" | "conversion_operator_declaration" => {
                (DefinitionKind::Method, "operator", false)
            }
            "property_declaration" => (DefinitionKind::Other, "property", false),
            "parameter" if variable == Some("record_declaration") => {
                (DefinitionKind::Other, "property", false)
            }
            "indexer_declaration" => (DefinitionKind::Other, "indexer", false),
            "event_declaration" => (DefinitionKind::Other, "event", false),
            "enum_member_declaration" => (DefinitionKind::Constant, "constant", false),
            "variable_declarator"
                if matches!(
                    variable,
                    Some("field_declaration" | "event_field_declaration")
                ) =>
            {
                (
                    DefinitionKind::Other,
                    if variable == Some("event_field_declaration") {
                        "event"
                    } else {
                        "field"
                    },
                    false,
                )
            }
            _ => return None,
        }
    };
    let name = if recovered_union {
        named_children(node)
            .find(|v| v.kind() == "variable_declaration")
            .and_then(|v| named_children(v).find(|v| v.kind() == "variable_declarator"))
            .and_then(|v| v.child_by_field_name("name"))
            .map(|n| simple(n, source))?
    } else if category == "indexer" {
        "this".into()
    } else if category == "operator" {
        format!(
            "operator {}",
            text(named("operator").or_else(|| named("type"))?, source)
        )
    } else {
        simple(named("name")?, source)
    };
    let header = if node.kind() == "variable_declarator" {
        parent?.parent()?
    } else {
        node
    };
    let modifiers: std::collections::BTreeSet<_> = named_children(header)
        .filter(|n| n.kind() == "modifier")
        .map(|n| text(n, source))
        .collect();
    let visibility = if node.kind() == "parameter" {
        "public".into()
    } else {
        super::walk::visibility(&modifiers, context, is_type)
    };
    let ty = named("returns").or_else(|| named("type")).or_else(|| {
        parent
            .filter(|_| node.kind() == "variable_declarator")?
            .child_by_field_name("type")
    });
    let parameters_node =
        named("parameters").or_else(|| named_children(node).find(|n| n.kind() == "parameter_list"));
    let parameters = parameters_node
        .map(|n| params(n, source))
        .unwrap_or_default();
    let generics = named_children(node)
        .find(|n| n.kind() == "type_parameter_list")
        .into_iter()
        .flat_map(named_children)
        .filter_map(|n| n.child_by_field_name("name"))
        .map(|n| simple(n, source))
        .collect();
    let bases = named_children(node)
        .filter(|n| matches!(n.kind(), "base_list" | "record_base"))
        .flat_map(named_children)
        .map(|n| text(n.child_by_field_name("type").unwrap_or(n), source))
        .collect();
    let types = ty
        .into_iter()
        .chain(
            parameters_node
                .into_iter()
                .flat_map(named_children)
                .filter_map(|p| p.child_by_field_name("type")),
        )
        .chain(
            named_children(node)
                .filter(|n| {
                    matches!(
                        n.kind(),
                        "type_parameter_constraints_clause" | "union_case_list"
                    )
                })
                .flat_map(named_children),
        )
        .map(|n| TypeExpression::from_node(n, source))
        .collect();
    let extension = parameters_node
        .and_then(|list| named_children(list).find(|p| p.kind() == "parameter"))
        .filter(|p| named_children(*p).any(|m| m.kind() == "modifier" && text(m, source) == "this"))
        .and_then(|p| p.child_by_field_name("type"))
        .map(|n| text(n, source))
        .or_else(|| context.extension.clone());
    let body = named("body")
        .or_else(|| named("value"))
        .or_else(|| named("accessors"));
    Some(Decl {
        fingerprint: None,
        execution: super::accessors::declaration(node, source),
        range: node.byte_range(),
        name,
        owner: context.owner.clone(),
        namespace: context.namespace.clone(),
        category: category.into(),
        kind,
        is_type,
        scope: context.scope.clone(),
        visibility,
        modifiers,
        parameters,
        generics,
        ty: ty.map(|n| text(n, source)).unwrap_or_default(),
        types,
        bases,
        extension,
        explicit: named_children(node)
            .find(|n| n.kind() == "explicit_interface_specifier")
            .map(|n| text(n, source).trim_end_matches('.').into()),
        body: body.map(|n| n.byte_range()),
    })
}

pub(in crate::languages::csharp) fn partial_union(node: Node<'_>, source: &[u8]) -> bool {
    matches!(
        node.kind(),
        "local_declaration_statement" | "field_declaration"
    ) && named_children(node).any(|n| n.kind() == "modifier" && text(n, source) == "partial")
        && named_children(node)
            .filter_map(|n| n.child_by_field_name("type"))
            .any(|ty| text(ty, source) == "union")
}

pub(super) fn params(node: Node<'_>, source: &[u8]) -> Vec<Parameter> {
    let mut result: Vec<_> = named_children(node)
        .filter(|n| n.kind() == "parameter")
        .map(|param| {
            let mut cursor = param.walk();
            let modifiers = param
                .children(&mut cursor)
                .map(|n| text(n, source))
                .collect::<Vec<_>>();
            Parameter {
                name: param
                    .child_by_field_name("name")
                    .map(|n| simple(n, source))
                    .unwrap_or_default(),
                ty: param
                    .child_by_field_name("type")
                    .map(|n| text(n, source))
                    .unwrap_or_default(),
                mode: modifiers
                    .iter()
                    .find(|s| matches!(s.as_str(), "ref" | "out" | "in"))
                    .cloned()
                    .unwrap_or_default(),
                optional: modifiers.iter().any(|s| s == "="),
                variadic: modifiers.iter().any(|s| s == "params"),
            }
        })
        .collect();
    if let (Some(name), Some(ty)) = (
        node.child_by_field_name("name"),
        node.child_by_field_name("type"),
    ) {
        result.push(Parameter {
            name: simple(name, source),
            ty: text(ty, source),
            mode: String::new(),
            optional: false,
            variadic: true,
        });
    }
    result
}
