use super::*;

pub(crate) fn package_name(nodes: &[Node<'_>], source: &[u8]) -> Vec<String> {
    let Some(header) = nodes
        .iter()
        .copied()
        .find(|node| node.kind() == "package_header")
    else {
        return Vec::new();
    };
    named_children(header)
        .into_iter()
        .find(|node| node.kind() == "identifier")
        .map(|node| split_name(&text(node, source)))
        .unwrap_or_default()
}

pub(crate) fn split_name(value: &str) -> Vec<String> {
    value
        .split('.')
        .filter(|part| !part.is_empty())
        .map(|part| part.trim_matches('`').to_owned())
        .collect()
}

pub(crate) fn collect_imports(
    root: Node<'_>,
    nodes: &[Node<'_>],
    source: &[u8],
    package: &[String],
) -> Vec<Import> {
    let mut imports = Vec::new();
    for node in nodes
        .iter()
        .copied()
        .filter(|node| node.kind() == "import_header")
    {
        let path_node = named_children(node)
            .into_iter()
            .find(|child| child.kind() == "identifier");
        let Some(path_node) = path_node else {
            continue;
        };
        let path = split_name(&text(path_node, source));
        if path.is_empty() {
            continue;
        }
        let wildcard = named_children(node)
            .into_iter()
            .any(|child| child.kind() == "wildcard_import");
        let alias = named_children(node)
            .into_iter()
            .find(|child| child.kind() == "import_alias")
            .and_then(|alias| named_children(alias).into_iter().next())
            .map(|alias| text(alias, source).trim_matches('`').to_owned());
        let imported_name = (!wildcard).then(|| path.last().cloned()).flatten();
        imports.push(Import {
            path,
            alias,
            source: None,
            imported_name,
            namespace: wildcard,
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
            module: ModulePath(package.to_vec()),
            scope_start: root.start_byte(),
            scope_end: root.end_byte(),
            is_public: false,
            context_id: 0,
        });
    }
    imports
}
