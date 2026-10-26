use super::*;

#[test]
fn opaque_dsl_names_do_not_change_scope_or_decision_metrics() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first.kt");
    let second = temp.path().join("second.kt");
    write(
        &first,
        concat!(
            "class Node { fun first(block: Node.() -> Unit) {} fun second(block: Node.() -> Unit) {} fun text(value: String) {} }\n",
            "fun render(node: Node, flag: Boolean) { node.first { second { if (flag) text(\"x\") } } }\n",
        ),
    );
    write(
        &second,
        "fun render(node: Any, flag: Boolean) { node.ExternalColumn { ExternalRow { if (flag) ExternalText(\"x\") } } }\n",
    );
    let report_a = report(&run("file", &first, None, Some("all")));
    let report_b = report(&run("file", &second, None, Some("all")));
    let a = &report_a["files"][0];
    let b = &report_b["files"][0];
    let callbacks_a: Vec<_> = a["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| {
            item["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("anonymous"))
        })
        .collect();
    let callbacks_b: Vec<_> = b["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| {
            item["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("anonymous"))
        })
        .collect();
    assert_eq!(callbacks_a.len(), callbacks_b.len());
    assert!(callbacks_a.len() >= 2, "nested DSL lambdas need scopes");
    for metric in [
        "nloc",
        "cyclomatic_complexity",
        "cyclomatic_density",
        "cognitive_complexity",
    ] {
        assert_eq!(function(a, "render")[metric], function(b, "render")[metric]);
    }
    for metric in [
        "distinct_operators",
        "distinct_operands",
        "total_operators",
        "total_operands",
        "vocabulary",
        "length",
        "volume",
    ] {
        assert_eq!(
            function(a, "render")["halstead"][metric],
            function(b, "render")["halstead"][metric],
            "opaque API names should not change the {metric} metric"
        );
    }
    assert_eq!(
        function(a, "render")["maintainability_index"]["score"],
        function(b, "render")["maintainability_index"]["score"]
    );
    let callback_scores_a: Vec<_> = callbacks_a
        .iter()
        .map(|item| item["cyclomatic_complexity"].clone())
        .collect();
    let callback_scores_b: Vec<_> = callbacks_b
        .iter()
        .map(|item| item["cyclomatic_complexity"].clone())
        .collect();
    assert_eq!(callback_scores_a, callback_scores_b);
    assert!(
        callbacks_a
            .iter()
            .any(|item| item["cyclomatic_complexity"].as_u64().unwrap() > 1)
    );
}
