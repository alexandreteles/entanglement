mod support;

use support::{function, only_file, report, run, write};

#[test]
fn c_metrics_use_written_bodies_and_runtime_conditions() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("logic.c");
    write(
        &path,
        r#"/* declarations */
int check(int, int);
int (*callback)(int);
int (*factory(int a))(int) { return callback; }
int check(int a, int b) {
#if defined(A) && defined(B)
    if (a && b) return 1;
#else
    switch (a) { case 0: return 3; default: break; }
#endif
    return a ? 1 : 0;
}
"#,
    );
    let result = report(&run("file", &path, None, None));
    let source = only_file(&result);
    assert_eq!(source["language"], "c");
    assert_eq!(source["functions"].as_array().unwrap().len(), 2);
    assert_eq!(function(source, "factory")["cyclomatic_complexity"], 1);
    assert_eq!(function(source, "check")["cyclomatic_complexity"], 5);
    assert_eq!(source["nloc"], 11);
}

#[test]
fn cpp_metrics_distinguish_overloads_and_lambda_bodies() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("model.C");
    write(
        &path,
        r#"namespace app {
struct Item {
    explicit Item(int n) : value(n) {}
    ~Item() = default;
    int get(int n) const { return n; }
    int get(double n) const;
    operator bool() const { return value != 0; }
    int value;
};
int Item::get(double n) const {
    auto convert = [](double v) { return v > 0 ? 1 : 0; };
    return convert(n);
}
}"#,
    );
    let result = report(&run("file", &path, None, None));
    let source = only_file(&result);
    assert_eq!(source["language"], "cpp");
    assert_eq!(source["functions"].as_array().unwrap().len(), 5);
    for name in [
        "app::Item::Item(int)",
        "app::Item::get(int) const",
        "app::Item::get(double) const",
        "app::Item::operator bool() const",
    ] {
        assert_eq!(function(source, name)["cyclomatic_complexity"], 1);
    }
    assert_eq!(function(source, "convert")["cyclomatic_complexity"], 2);
}

#[test]
fn unevaluated_expressions_have_no_runtime_decisions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sizes.cpp");
    write(
        &path,
        "int check(int x) { return sizeof(x && x) + noexcept(x || x); }\n",
    );
    let report = report(&run("file", &path, None, None));
    let check = function(only_file(&report), "check(int)");
    assert_eq!(check["cyclomatic_complexity"], 1);
    assert_eq!(check["cognitive_complexity"], 0);
}
