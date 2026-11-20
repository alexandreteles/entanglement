mod support;
use serde_json::{Value, json};
use support::{file, function, report, run, write};

fn reference<'a>(source: &'a Value, name: &str) -> &'a Value {
    &source["resolution"]
        .as_array()
        .unwrap()
        .iter()
        .find(|reference| reference["path"].as_array().unwrap().last().unwrap() == name)
        .unwrap()["resolution"]
}

#[test]
fn include_order_and_conflicting_builds_control_which_header_body_is_visible() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    write(
        &root.join("a/api.h"),
        "static inline int api(void) { return 1; }\n",
    );
    write(
        &root.join("b/api.h"),
        "static inline int api(void) { return 2; }\n",
    );
    write(
        &root.join("client.c"),
        "#include <api.h>\nint client(void) { return api(); }\n",
    );
    let entry = |a, b| json!({"file":"client.c","directory": root,"arguments":["cc","-I",a,"-I",b,"-c","client.c"]});
    let run_with = |entries: Value| {
        write(&root.join("compile_commands.json"), entries.to_string());
        report(&run("repo", root, None, None))
    };
    let first = run_with(json!([entry("b", "a")]));
    assert!(
        reference(file(&first, "client.c"), "api")["symbols"]["file"]
            .as_str()
            .unwrap()
            .ends_with("b/api.h")
    );
    let conflicting = run_with(json!([entry("b", "a"), entry("a", "b")]));
    assert_eq!(
        reference(file(&conflicting, "client.c"), "api")["status"],
        "ambiguous"
    );
}

#[test]
fn mutually_exclusive_configurations_cannot_create_a_recursion_cycle() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    write(
        &root.join("config.c"),
        "int a(void); int b(void);\nint a(void) {\n#if MODE\nreturn b();\n#endif\nreturn 0;\n}\nint b(void) {\n#if !MODE\nreturn a();\n#endif\nreturn 0;\n}\n",
    );
    let entries:Vec<_>=[0,1].iter().map(|mode|json!({"file":"config.c","directory":root,"arguments":["cc",format!("-DMODE={mode}"),"config.c"]})).collect();
    write(
        &root.join("compile_commands.json"),
        json!(entries).to_string(),
    );
    let result = report(&run("repo", root, None, None));
    for name in ["a", "b"] {
        assert_eq!(
            function(file(&result, "config.c"), name)["cognitive_complexity"],
            0
        );
    }
}

#[test]
fn explicit_header_language_takes_precedence_over_includers() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    write(&root.join("api.h"), "int check(void);\n");
    write(&root.join("client.c"), "#include \"api.h\"\n");
    write(
        &root.join("compile_commands.json"),
        json!([{"file":"api.h","directory":root,"command":"c++ -x c++-header api.h"}]).to_string(),
    );
    let result = report(&run("repo", root, None, Some("nloc")));
    assert_eq!(file(&result, "api.h")["language"], "cpp");
}
