mod support;
use support::{only_file, references_to, report, run, write};

#[test]
fn statement_bindings_end_before_the_next_statement() {
    let project = tempfile::tempdir().unwrap();
    let path = project.path().join("scope.cpp");
    let source = r#"int i, err, key, value;
struct Pair { int first; int second; };
int read(Pair (&pairs)[2], int n) {
    for (int i = 0; i < n; ++i) {}
    if (int key = n) { ++key; }
    try {} catch (int err) { ++err; }
    for (auto [key, value] : pairs) { ++key; ++value; }
    { auto [key, value] = pairs[0]; ++key; ++value; }
    return i + err + key + value;
}
"#;
    write(&path, source);
    let report = report(&run("file", &path, None, None));
    let file = only_file(&report);
    for name in ["i", "err", "key", "value"] {
        let uses = references_to(file, name);
        assert_eq!(uses.len(), 1, "{name}: {uses:?}");
        assert_eq!(uses[0]["resolution"]["symbols"]["kind"], "static");
    }
}

#[test]
fn qualified_lookup_preserves_the_global_and_nested_namespaces() {
    let project = tempfile::tempdir().unwrap();
    let path = project.path().join("names.cpp");
    write(
        &path,
        "int f() {return 1;}\nnamespace a::b { int f(){return 2;} int inside(){ int f = 0; return ::f(); } }\nint use(){return a::b::f();}\n",
    );
    let report = report(&run("file", &path, None, None));
    let uses = references_to(only_file(&report), "f");
    assert_eq!(uses.len(), 2);
    assert_eq!(
        uses[0]["resolution"]["symbols"]["module"],
        serde_json::json!([])
    );
    assert_eq!(
        uses[1]["resolution"]["symbols"]["module"],
        serde_json::json!(["a", "b"])
    );
}
