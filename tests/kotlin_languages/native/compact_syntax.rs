use super::*;

#[test]
fn compact_class_companion_and_generic_dsl_scopes_match_multiline_source() {
    let temp = tempfile::tempdir().unwrap();
    let compact_path = temp.path().join("compact.kt");
    let multiline_path = temp.path().join("multiline.kt");
    write(
        &compact_path,
        concat!(
            "class C(flag:Boolean){/* } fun hidden() = if (true) 1 else 0; */",
            "val note=\"brace } { fun ghost() = if (true) 1 else 0\";",
            "fun direct(flag:Boolean)=if(flag)1 else 0;",
            "companion object{fun companion(flag:Boolean)=if(flag)1 else 0}}\n",
            "class Builder{fun child()=1}\n",
            "fun <T> withValue(value:T,block:T.()->Unit){}\n",
            "fun use(builder:Builder,flag:Boolean){withValue(builder){if(flag)child()}}\n",
        ),
    );
    write(
        &multiline_path,
        concat!(
            "class C(flag: Boolean) {\n",
            "    /* } fun hidden() = if (true) 1 else 0; */\n",
            "    val note = \"brace } { fun ghost() = if (true) 1 else 0\"\n",
            "    fun direct(flag: Boolean) = if (flag) 1 else 0\n",
            "    companion object {\n",
            "        fun companion(flag: Boolean) = if (flag) 1 else 0\n",
            "    }\n",
            "}\n",
            "class Builder { fun child() = 1 }\n",
            "fun <T> withValue(value: T, block: T.() -> Unit) {}\n",
            "fun use(builder: Builder, flag: Boolean) {\n",
            "    withValue(builder) {\n",
            "        if (flag) child()\n",
            "    }\n",
            "}\n",
        ),
    );

    let compact_report = report(&run("file", &compact_path, None, Some("all")));
    let multiline_report = report(&run("file", &multiline_path, None, Some("all")));
    let compact = &compact_report["files"][0];
    let multiline = &multiline_report["files"][0];
    let compact_scope_labels = compact["functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            format!(
                "{}@{}..{}",
                item["name"].as_str().unwrap_or("<unnamed>"),
                item["start_byte"],
                item["end_byte"]
            )
        })
        .collect::<Vec<_>>();
    assert!(
        compact_scope_labels
            .iter()
            .any(|label| label.starts_with("C@")),
        "the compact primary constructor should be reported; scopes: {compact_scope_labels:?}"
    );
    for name in ["C", "direct", "companion", "child", "withValue", "use"] {
        let compact_scope = function(compact, name);
        let multiline_scope = function(multiline, name);
        for metric in ["cyclomatic_complexity", "cognitive_complexity"] {
            assert_eq!(
                compact_scope[metric], multiline_scope[metric],
                "compact owner layout changed {name}.{metric}"
            );
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
                compact_scope["halstead"][metric], multiline_scope["halstead"][metric],
                "compact owner layout changed {name} Halstead {metric}"
            );
        }
    }
    for source in [compact, multiline] {
        let functions = source["functions"].as_array().unwrap();
        assert!(
            !functions
                .iter()
                .any(|item| { matches!(item["name"].as_str(), Some("hidden" | "ghost")) })
        );
        let callbacks: Vec<_> = functions
            .iter()
            .filter(|item| {
                item["name"]
                    .as_str()
                    .is_some_and(|name| name.starts_with("anonymous@"))
            })
            .collect();
        assert_eq!(callbacks.len(), 1, "one generic DSL callback scope");
        assert_eq!(callbacks[0]["cyclomatic_complexity"], 2);
        assert_eq!(callbacks[0]["cognitive_complexity"], 2);
        assert!(!has_operand(&callbacks[0]["halstead"], "ghost"));
        assert!(!has_operand(&callbacks[0]["halstead"], "hidden"));
    }
    let compact_callbacks: Vec<_> = compact["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| {
            item["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("anonymous@"))
        })
        .collect();
    let multiline_callbacks: Vec<_> = multiline["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| {
            item["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("anonymous@"))
        })
        .collect();
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
            compact_callbacks[0]["halstead"][metric], multiline_callbacks[0]["halstead"][metric],
            "compact generic callback changed Halstead {metric}"
        );
    }
}
