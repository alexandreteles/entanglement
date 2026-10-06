use tree_sitter::Node;

use crate::model::Export;

use super::{module_facts::export_fact, syntax};

pub(super) fn parse_namespace_export(
    statement: Node<'_>,
    namespace: Node<'_>,
    module_source: Option<String>,
    source: &[u8],
) -> Vec<Export> {
    let (Some(module_source), Some(exported_name)) = (
        module_source,
        namespace
            .named_child(0)
            .and_then(|name| syntax::node_name(name, source)),
    ) else {
        return Vec::new();
    };
    vec![export_fact(
        statement,
        Some(module_source),
        None,
        None,
        exported_name,
        true,
    )]
}

pub(super) fn parse_export_clause(
    statement: Node<'_>,
    clause: Node<'_>,
    module_source: Option<String>,
    source: &[u8],
) -> Vec<Export> {
    let mut exports = Vec::new();
    for index in 0..clause.named_child_count() {
        let Some(specifier) = syntax::named_child_at(clause, index) else {
            continue;
        };
        if specifier.kind() != "export_specifier" {
            continue;
        }
        if let Some(export) =
            export_specifier(statement, specifier, module_source.as_deref(), source)
        {
            exports.push(export);
        }
    }
    if exports.is_empty() && module_source.is_some() && syntax::has_wildcard(statement) {
        return parse_wildcard_export(statement, module_source);
    }
    exports
}

fn export_specifier(
    statement: Node<'_>,
    specifier: Node<'_>,
    module_source: Option<&str>,
    source: &[u8],
) -> Option<Export> {
    let name = specifier
        .child_by_field_name("name")
        .and_then(|name| syntax::node_name(name, source))?;
    let exported_name = specifier
        .child_by_field_name("alias")
        .and_then(|alias| syntax::node_name(alias, source))
        .unwrap_or_else(|| name.clone());
    let is_reexport = module_source.is_some();
    Some(export_fact(
        statement,
        module_source.map(str::to_owned),
        is_reexport.then_some(name.clone()),
        (!is_reexport).then_some(name),
        exported_name,
        false,
    ))
}

pub(super) fn parse_wildcard_export(
    statement: Node<'_>,
    module_source: Option<String>,
) -> Vec<Export> {
    module_source
        .map(|source| {
            vec![export_fact(
                statement,
                Some(source),
                None,
                None,
                "*".to_owned(),
                false,
            )]
        })
        .unwrap_or_default()
}
