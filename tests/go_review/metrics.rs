use super::{file, function, kinds, report, resolutions, run, status, write};

#[test]
fn empty_multiways_and_unconditional_loop_headers_have_no_cc_decision() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("metrics.go");
    write(
        &path,
        r#"package metrics
func EmptySwitch() { switch {} }
func DefaultSwitch() { switch { default: } }
func EmptySelect() { select {} }
func DefaultSelect() { select { default: } }
func Bare() { for {} }
func Clauses() { for ;; {} }
func Update() { for i := 0; ; i++ {} }
func Comment() { for /* condition? */ {} }
func Conditional(x bool) { for x {} }
func ThreeClause() { for i := 0; i < 2; i++ {} }
func Range() { for range 2 {} }
func Cases(x int) { switch x { case 1, 2: case 3: default: } }
"#,
    );
    let output = report(&run(&["file"], &path, None));
    let f = &output["files"][0];
    for name in [
        "EmptySwitch",
        "DefaultSwitch",
        "EmptySelect",
        "DefaultSelect",
        "Bare",
        "Clauses",
        "Update",
        "Comment",
    ] {
        assert_eq!(function(f, name)["cyclomatic_complexity"], 1, "{name}");
        assert_eq!(function(f, name)["cognitive_complexity"], 1, "{name}");
    }
    for name in ["Conditional", "ThreeClause", "Range"] {
        assert_eq!(function(f, name)["cyclomatic_complexity"], 2, "{name}");
    }
    let cases = function(f, "Cases");
    assert_eq!(cases["cyclomatic_complexity"], 3);
    assert_eq!(cases["contributions"].as_array().unwrap().len(), 3);
}

#[test]
fn parentheses_keep_direct_recursion_but_indexed_calls_do_not_invent_edges() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("calls.go");
    write(
        &path,
        r#"package calls
func F(n int) int { if n == 0 { return 0 }; return ((F))(n-1) }
func G[T any](x T) { G[T](x) }
func GP[T any](x T) { ((GP[T]))(x) }
func Indexed(fs []func()) { fs[0]() }
func Deferred() { defer (Deferred)() }
func Concurrent() { go (Concurrent)() }
"#,
    );
    let output = report(&run(&["file"], &path, None));
    let f = &output["files"][0];
    assert_eq!(function(f, "F")["cognitive_complexity"], 2);
    for name in ["G", "GP", "Deferred", "Concurrent"] {
        assert!(
            kinds(function(f, name)).contains(&("recursion".into(), 1)),
            "{name}"
        );
    }
    assert_eq!(function(f, "Indexed")["cognitive_complexity"], 0);
    assert_eq!(status(&resolutions(f), "fs"), ["unresolved"]);
}

#[test]
fn parenthesized_package_calls_keep_their_symbol_and_nested_function_owner() {
    let temp = tempfile::tempdir().unwrap();
    write(&temp.path().join("go.mod"), "module example.com/app\n");
    write(
        &temp.path().join("dep/dep.go"),
        "package dep\nfunc F() {}\n",
    );
    let path = temp.path().join("main.go");
    write(
        &path,
        r#"package app
import "example.com/app/dep"
func Outer() { f := func() { (dep.F)() }; f() }
"#,
    );
    let output = report(&run(&["repo"], temp.path(), None));
    let f = file(&output, "main.go");
    assert_eq!(status(&resolutions(f), "dep.F"), ["exact dep.go:F"]);
    assert_eq!(function(f, "Outer")["cyclomatic_complexity"], 1);
    assert_eq!(function(f, "f")["cyclomatic_complexity"], 1);
}
