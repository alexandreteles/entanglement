use tree_sitter::Node;

use crate::model::Export;

use super::{module_facts::export_fact, reexports, syntax};

/// Convert a JavaScript or TypeScript export statement into export facts.
pub(super) fn parse_export(node: Node<'_>, source: &[u8]) -> Vec<Export> {
    let module_source = node
        .child_by_field_name("source")
        .and_then(|source_node| syntax::normalize_string_node(source_node, source));
    if let Some(namespace) = syntax::named_child(node, "namespace_export") {
        return reexports::parse_namespace_export(node, namespace, module_source, source);
    }
    if let Some(clause) = syntax::named_child(node, "export_clause") {
        return reexports::parse_export_clause(node, clause, module_source, source);
    }
    if module_source.is_some() && syntax::has_wildcard(node) {
        return reexports::parse_wildcard_export(node, module_source);
    }
    if syntax::has_default_keyword(node) {
        return parse_default_export(node, source);
    }
    parse_declaration_export(node, source)
}

fn parse_default_export(statement: Node<'_>, source: &[u8]) -> Vec<Export> {
    let local_names = statement
        .child_by_field_name("declaration")
        .map(|declaration| declaration_names(declaration, source))
        .unwrap_or_else(|| {
            statement
                .child_by_field_name("value")
                .filter(|value| value.kind() == "identifier")
                .and_then(|value| syntax::node_text(value, source))
                .into_iter()
                .collect()
        });
    if local_names.is_empty() {
        return vec![export_fact(
            statement,
            None,
            None,
            Some("default".to_owned()),
            "default".to_owned(),
            false,
        )];
    }
    local_names
        .into_iter()
        .map(|local| {
            export_fact(
                statement,
                None,
                None,
                Some(local),
                "default".to_owned(),
                false,
            )
        })
        .collect()
}

fn parse_declaration_export(statement: Node<'_>, source: &[u8]) -> Vec<Export> {
    let Some(declaration) = statement.child_by_field_name("declaration") else {
        return Vec::new();
    };
    declaration_names(declaration, source)
        .into_iter()
        .map(|name| export_fact(statement, None, None, Some(name.clone()), name, false))
        .collect()
}

fn declaration_names(declaration: Node<'_>, source: &[u8]) -> Vec<String> {
    match declaration.kind() {
        "lexical_declaration" | "variable_declaration" => variable_names(declaration, source),
        _ => declaration
            .child_by_field_name("name")
            .and_then(|name| syntax::node_name(name, source))
            .into_iter()
            .collect(),
    }
}

pub(super) fn default_callable(statement: Node<'_>) -> Option<Node<'_>> {
    if !syntax::has_default_keyword(statement) {
        return None;
    }
    let value = statement
        .child_by_field_name("declaration")
        .or_else(|| statement.child_by_field_name("value"))?;
    let callable = unwrap_callable(value)?;
    matches!(
        callable.kind(),
        "arrow_function"
            | "function_expression"
            | "function_declaration"
            | "generator_function"
            | "generator_function_declaration"
    )
    .then_some(callable)
}

fn unwrap_callable(mut node: Node<'_>) -> Option<Node<'_>> {
    loop {
        match node.kind() {
            "parenthesized_expression" | "as_expression" | "satisfies_expression" => {
                node = node
                    .child_by_field_name("expression")
                    .or_else(|| node.named_child(0))?;
            }
            _ => return Some(node),
        }
    }
}

fn variable_names(declaration: Node<'_>, source: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    for index in 0..declaration.named_child_count() {
        let Some(declarator) = syntax::named_child_at(declaration, index) else {
            continue;
        };
        if declarator.kind() == "variable_declarator"
            && let Some(pattern) = declarator.child_by_field_name("name")
        {
            names.extend(
                super::facts::binding_names(pattern, source)
                    .into_iter()
                    .map(|(name, _)| name),
            );
        }
    }
    names
}
