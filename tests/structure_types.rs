#[path = "../src/structure/types.rs"]
mod types;

fn rust_type(text: &str) -> types::TypeExpression {
    let source = format!("type Alias = {text};");
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(&source, None).unwrap();
    let node = tree
        .root_node()
        .named_child(0)
        .unwrap()
        .child_by_field_name("type")
        .unwrap();
    types::TypeExpression::from_node(node, source.as_bytes())
}

#[test]
fn nested_types_keep_each_written_layer() {
    let simple = rust_type("Vec<u8>");
    let nested = rust_type("Vec<Vec<Vec<u8>>>");
    assert_ne!(simple, nested);
    assert_eq!(nested.text, "Vec<Vec<Vec<u8>>>");
    assert_eq!(nested.form, "generic_type");
}

#[test]
fn qualified_names_keep_their_path_structure() {
    let expression = rust_type("std::vec::Vec<u8>");
    assert_eq!(expression.form, "generic_type");
    assert_eq!(expression.children[0].text, "std::vec::Vec");
}

#[test]
fn type_comparison_ignores_spacing_and_comments() {
    assert_eq!(
        rust_type("Vec<u8>").semantic_key(),
        rust_type("Vec < /* layer */ u8 >").semantic_key()
    );
}

#[test]
fn rust_lifetimes_keep_tokens_without_source_spacing() {
    assert_eq!(
        rust_type("&'a Vec<u8>").semantic_key(),
        rust_type("& 'a Vec < u8 >").semantic_key()
    );
}
