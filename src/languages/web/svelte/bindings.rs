use tree_sitter::{Language, Node, Parser};

pub(super) fn parse_names(node: Node<'_>, source: &[u8], declaration: bool) -> Vec<String> {
    let body = String::from_utf8_lossy(&source[node.byte_range()]);
    let typed = node
        .named_child(0)
        .is_some_and(|child| child.kind() == "ts");
    let wrapped = if declaration {
        format!("let {body};")
    } else {
        format!("function __entanglement({body}) {{}}")
    };
    let language: Language = if typed {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    } else {
        tree_sitter_javascript::LANGUAGE.into()
    };
    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(&wrapped, None) else {
        return Vec::new();
    };
    let kind = if declaration {
        "variable_declarator"
    } else {
        "formal_parameters"
    };
    let Some(pattern) = find_kind(tree.root_node(), kind).and_then(|node| {
        declaration
            .then(|| node.child_by_field_name("name"))
            .flatten()
            .or(Some(node))
    }) else {
        return Vec::new();
    };
    super::super::facts::binding_names(pattern, wrapped.as_bytes())
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

fn find_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    if node.kind() == kind {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find_map(|child| find_kind(child, kind))
}
