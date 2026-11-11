use tree_sitter::Node;

/// Constructs the C grammar never produces, so they survive error recovery as evidence of C++.
const CPP_ONLY_KINDS: &[&str] = &[
    "access_specifier",
    "alias_declaration",
    "class_specifier",
    "lambda_expression",
    "namespace_definition",
    "qualified_identifier",
    "reference_declarator",
    "template_declaration",
    "using_declaration",
];

/// Whether a tree parsed with the C++ grammar holds syntax that only C++ has.
pub(crate) fn has_cpp_syntax(root: Node<'_>) -> bool {
    let mut cursor = root.walk();
    loop {
        if CPP_ONLY_KINDS.contains(&cursor.node().kind()) {
            return true;
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return false;
            }
        }
    }
}
