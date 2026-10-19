use super::SyntaxGraph;

#[test]
fn indexed_nodes_keep_tree_order_and_parent_links() {
    let mut parser = tree_sitter::Parser::new();
    let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    parser.set_language(&language).expect("Rust grammar loads");
    let source = b"fn first() {}\nfn second() {}";
    let tree = parser.parse(source, None).expect("source parses");
    let root = tree.root_node();
    let graph = SyntaxGraph::from_tree(root, source);
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        assert_eq!(graph.node(child.id()), Some(child));
        assert_eq!(graph.parent(child.id()), Some(root.id()));
    }
}
