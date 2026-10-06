use tree_sitter::Node;

use crate::model::Import;

use super::{module_facts::import_fact, syntax};

/// Convert a JavaScript or TypeScript import statement into binding facts.
pub(super) fn parse_import(node: Node<'_>, source: &[u8]) -> Vec<Import> {
    let Some(module_source) = node
        .child_by_field_name("source")
        .and_then(|source_node| syntax::normalize_string_node(source_node, source))
    else {
        return Vec::new();
    };
    let Some(clause) = syntax::named_child(node, "import_clause") else {
        return vec![import_fact(node, &module_source, None, None, false)];
    };

    let mut imports = Vec::new();
    for index in 0..clause.named_child_count() {
        let Some(child) = syntax::named_child_at(clause, index) else {
            continue;
        };
        match child.kind() {
            "identifier" => imports.push(import_fact(
                node,
                &module_source,
                Some("default".to_owned()),
                syntax::node_text(child, source),
                false,
            )),
            "namespace_import" => {
                if let Some(binding) = child
                    .named_child(0)
                    .and_then(|name| syntax::node_text(name, source))
                {
                    imports.push(import_fact(node, &module_source, None, Some(binding), true));
                }
            }
            "named_imports" => {
                collect_named_imports(child, node, &module_source, source, &mut imports)
            }
            _ => {}
        }
    }
    imports
}

fn collect_named_imports(
    named: Node<'_>,
    statement: Node<'_>,
    module_source: &str,
    source: &[u8],
    imports: &mut Vec<Import>,
) {
    for index in 0..named.named_child_count() {
        let Some(specifier) = syntax::named_child_at(named, index) else {
            continue;
        };
        if specifier.kind() != "import_specifier" {
            continue;
        }
        let imported = specifier
            .child_by_field_name("name")
            .and_then(|name| syntax::node_name(name, source));
        let local = specifier
            .child_by_field_name("alias")
            .or_else(|| specifier.child_by_field_name("name"))
            .and_then(|name| syntax::node_name(name, source));
        if let (Some(imported), Some(local)) = (imported, local) {
            imports.push(import_fact(
                statement,
                module_source,
                Some(imported),
                Some(local),
                false,
            ));
        }
    }
}
