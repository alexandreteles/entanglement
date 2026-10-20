use super::{ScopeIndex, capture, scope};

#[test]
fn cached_ranges_precede_names_and_preserve_nearest_scope_identity() {
    let source = concat!(
        "context(String)\n",
        "fun run(flag: Boolean = defaultCall()) {\n",
        "    val bound = { boundCall() }\n",
        "    consume { callbackCall() }\n",
        "}\n",
        "println(scriptCall())\n",
    );
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_kotlin::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let index = ScopeIndex::new(tree.root_node(), source.as_bytes());
    let range_of = |text: &str| {
        let start = source.find(text).unwrap();
        start..start + text.len()
    };
    let bound = range_of("{ boundCall() }");
    let callback = range_of("{ callbackCall() }");
    let outer = 0..source.find("\n}\n").unwrap() + 2;
    let script = range_of("println(scriptCall())");

    // Ownership is available before query capture supplies display names.
    for (marker, expected) in [
        ("defaultCall", outer.clone()),
        ("boundCall", bound.clone()),
        ("callbackCall", callback.clone()),
        ("scriptCall", script.clone()),
    ] {
        let range = range_of(marker);
        let node = index
            .nodes()
            .iter()
            .find(|node| node.byte_range() == range)
            .unwrap();
        assert_eq!(index.metric_scope_range_for(*node), Some(expected));
    }
    let lambda = index
        .nodes()
        .iter()
        .find(|node| node.kind() == "lambda_literal" && node.byte_range() == callback)
        .unwrap();
    assert_eq!(
        index.metric_scope_range_for(*lambda),
        Some(callback.clone())
    );
    assert_eq!(index.metric_scope_range_for(tree.root_node()), None);

    let query_functions = [bound.clone(), callback.clone()]
        .into_iter()
        .map(|range| scope("run".into(), range.start, range.end, source.as_bytes()))
        .collect();
    let functions = capture(&index, query_functions);
    for (name, range) in [
        ("run", outer),
        ("bound", bound),
        ("run", callback),
        ("script@6", script),
    ] {
        assert!(
            functions
                .iter()
                .any(|function| function.name == name && function.range == range)
        );
    }
}
