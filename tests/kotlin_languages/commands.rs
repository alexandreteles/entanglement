use super::*;

#[test]
fn kotlin_file_selection_includes_gradle_kotlin_scripts() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.kt");
    write(
        &source,
        "fun selectedFromKotlin(flag: Boolean) = if (flag) \"yes\" else \"no\"\n",
    );
    let selected = report(&run("file", &source, None, Some("cc")));
    assert_eq!(selected["files"][0]["language"], "kotlin");
    assert!(
        selected["files"][0]["functions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["name"] == "selectedFromKotlin")
    );

    let script = temp.path().join("build.gradle.kts");
    write(
        &script,
        concat!(
            "val selected = true\n",
            "if (selected && args.isNotEmpty()) println(\"Gradle script\")\n",
            "tasks.register(\"verify\") { doLast { if (selected) println(\"verified\") } }\n",
        ),
    );
    let result = report(&run("file", &script, None, Some("cc")));
    assert_eq!(result["files"][0]["language"], "kotlin");
    let functions = result["files"][0]["functions"].as_array().unwrap();
    let script_scope = functions
        .iter()
        .find(|item| {
            item["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("script@"))
        })
        .expect("script body scope");
    assert!(script_scope["cyclomatic_complexity"].as_u64().unwrap() >= 3);
    assert!(functions.iter().any(|item| {
        item["name"]
            .as_str()
            .is_some_and(|name| name.starts_with("anonymous@"))
    }));
}
