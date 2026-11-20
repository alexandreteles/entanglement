mod support;
use serde_json::Value;
use support::{file, function, patch, report, run, write};

fn call<'a>(file: &'a Value, name: &str) -> &'a Value {
    file["resolution"]
        .as_array()
        .unwrap()
        .iter()
        .find(|reference| reference["path"].as_array().unwrap().last().unwrap() == name)
        .unwrap_or_else(|| panic!("missing reference to {name}"))
}

#[test]
fn a_cpp_caller_links_to_a_c_body_through_a_shared_header() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    write(
        &root.join("api.h"),
        "#ifdef __cplusplus\nextern \"C\" {\n#endif\nint left(int);\nint right(int);\n#ifdef __cplusplus\n}\n#endif\n",
    );
    write(
        &root.join("left.c"),
        "#include \"api.h\"\nint left(int x) { return right(x); }\n",
    );
    write(
        &root.join("right.cpp"),
        "#include \"api.h\"\nextern \"C\" int right(int x) { return left(x); }\n",
    );
    let result = report(&run("repo", root, None, None));
    for (path, name, other, target) in [
        ("left.c", "left", "right", "right.cpp"),
        ("right.cpp", "right(int)", "left", "left.c"),
    ] {
        let source = file(&result, path);
        assert_eq!(function(source, name)["cognitive_complexity"], 1);
        let resolution = &call(source, other)["resolution"];
        assert!(
            resolution["symbols"]["file"]
                .as_str()
                .unwrap()
                .ends_with(target)
        );
    }
}

#[test]
fn overloads_and_static_functions_keep_their_own_identity() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    write(
        &root.join("a.cpp"),
        "static int get(int n) { return get(n); }\nint get(double n) { return get(1); }\nint via(int (*get)(int)) { return get(2); }\n",
    );
    write(&root.join("b.cpp"), "static int get(int n) { return n; }\n");
    let result = report(&run("repo", root, None, None));
    let source = file(&result, "a.cpp");
    assert_eq!(function(source, "get(int)")["cognitive_complexity"], 1);
    assert_eq!(function(source, "get(double)")["cognitive_complexity"], 0);
    assert_eq!(
        source["resolution"]
            .as_array()
            .unwrap()
            .iter()
            .rfind(|item| item["path"][0] == "get")
            .unwrap()["resolution"]["status"],
        "unresolved"
    );
}

#[test]
fn a_header_edit_changes_recursion_in_an_untouched_caller() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let before = "int client(void);\nstatic inline int api(void) { return client(); }\n";
    let after = "int client(void);\nstatic inline int api(void) { return 0; }\n";
    write(&root.join("api.h"), before);
    write(
        &root.join("client.c"),
        "#include \"api.h\"\nint client(void) { return api(); }\n",
    );
    let original = report(&run("repo", root, None, None));
    assert_eq!(
        function(file(&original, "client.c"), "client")["cognitive_complexity"],
        1
    );
    let diff = root.join("proposed.diff");
    write(&diff, patch("api.h", Some(before), Some(after)));
    let result = report(&run("candidate", root, Some(&diff), None));
    assert_eq!(
        function(file(&result, "client.c"), "client")["cognitive_complexity"],
        0
    );
}

#[test]
fn virtual_calls_and_implicit_overrides_do_not_prove_recursion() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.cpp");
    write(
        &path,
        "struct Base { virtual int f() { return f(); } }; struct Child : Base { int f() { return f(); } }; struct Plain { int f() { return f(); } };\n",
    );
    let result = report(&run("file", &path, None, None));
    for (name, score) in [("Base::f()", 0), ("Child::f()", 0), ("Plain::f()", 1)] {
        assert_eq!(
            function(file(&result, "model.cpp"), name)["cognitive_complexity"],
            score,
            "{name}"
        );
    }
}
