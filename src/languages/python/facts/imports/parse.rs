use tree_sitter::Node;

use super::super::syntax::{children, field, text};

pub(super) struct ImportedName {
    pub source: String,
    pub imported_name: Option<String>,
    pub binding: String,
    pub alias: Option<String>,
    pub namespace: bool,
    pub wildcard: bool,
}

pub(super) fn imports(statement: Node<'_>, source: &[u8]) -> Vec<ImportedName> {
    match statement.kind() {
        "import_statement" => children(statement)
            .into_iter()
            .map(|node| imported_module(node, source))
            .collect(),
        "import_from_statement" => import_from(statement, source),
        _ => Vec::new(),
    }
}

fn imported_module(node: Node<'_>, source: &[u8]) -> ImportedName {
    let name = if node.kind() == "aliased_import" {
        field(node, "name")
    } else {
        Some(node)
    };
    let module = name.map(|item| text(item, source)).unwrap_or_default();
    let alias = field(node, "alias").map(|item| text(item, source));
    let binding = alias
        .clone()
        .unwrap_or_else(|| module.split('.').next().unwrap_or(&module).to_owned());
    let source = if alias.is_some() {
        module
    } else {
        binding.clone()
    };
    ImportedName {
        source,
        imported_name: None,
        binding,
        alias,
        namespace: true,
        wildcard: false,
    }
}

fn import_from(statement: Node<'_>, source: &[u8]) -> Vec<ImportedName> {
    let module = field(statement, "module_name")
        .map(|node| text(node, source))
        .unwrap_or_default();
    let module_id = field(statement, "module_name").map(|node| node.id());
    children(statement)
        .into_iter()
        .filter(|node| Some(node.id()) != module_id)
        .map(|node| imported_member(node, &module, source))
        .collect()
}

fn imported_member(node: Node<'_>, module: &str, source: &[u8]) -> ImportedName {
    if node.kind() == "wildcard_import" {
        return ImportedName {
            source: module.to_owned(),
            imported_name: Some("*".to_owned()),
            binding: "*".to_owned(),
            alias: None,
            namespace: false,
            wildcard: true,
        };
    }
    let (name_node, alias_node) = if node.kind() == "aliased_import" {
        (field(node, "name"), field(node, "alias"))
    } else {
        (Some(node), None)
    };
    let imported_name = name_node.map(|item| text(item, source)).unwrap_or_default();
    let alias = alias_node.map(|item| text(item, source));
    ImportedName {
        source: module.to_owned(),
        imported_name: Some(imported_name.clone()),
        binding: alias.clone().unwrap_or(imported_name),
        alias,
        namespace: false,
        wildcard: false,
    }
}
