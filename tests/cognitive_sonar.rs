use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn analyze(path: &Path) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_entanglement"))
        .args(["--format", "json", "file"])
        .arg(path)
        .output()
        .expect("run file analysis");
    assert!(
        output.status.success(),
        "entanglement failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("parse JSON report")
}

fn function<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["files"][0]["functions"]
        .as_array()
        .expect("function metrics")
        .iter()
        .find(|function| function["name"] == name)
        .unwrap_or_else(|| panic!("missing function {name}"))
}

#[test]
fn rust_syntax_cases_match_sonar_cognitive_complexity_examples() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let source = temp.path().join("cognitive.rs");
    std::fs::write(
        &source,
        r#"
macro_rules! m {
    ($($tokens:tt)*) => { $($tokens)* };
}

fn sumOfPrimes(max: usize) -> usize {
    let mut total = 0;
    'outer: for i in 1..=max {
        for j in 2..i {
            if i % j == 0 {
                continue 'outer;
            }
        }
        total += i;
    }
    total
}

fn getWords(number: usize) -> &'static str {
    match number {
        1 => "one",
        2 => "a couple",
        3 => "a few",
        _ => "lots",
    }
}

fn nested_macro_injection(a: bool, b: bool) {
    if a { m! { m! { if b {} } } }
}

fn closure_inside_if(a: bool) {
    if a { let _closure = || if a {}; }
}

fn async_block() {
    let _future = async { if true {} };
}

fn if_inside_if_condition(a: bool) {
    if (if a { true } else { false }) {}
}

fn boolean_runs(a: bool, b: bool, c: bool, d: bool) {
    if a && b || c {}
    let _ = a && (b || c) && d;
    let _ = !(a && b) || c;
}

fn else_if_nesting(a: bool, b: bool, c: bool) {
    loop { if a {} else if b { if c {} } break; }
}

fn match_guard(value: Option<i32>) {
    match value { Some(v) if v > 0 => {}, _ => {} }
}

fn let_else_condition_nested(a: bool) {
    let Some(v) = (if a { Some(1) } else { None }) else { return; };
}

fn let_chain(value: Option<i32>, b: bool, c: bool) {
    if let Some(x) = value && x > 0 && (b || c) {}
}

fn labeled_break() {
    'outer: loop { loop { break 'outer; } }
}

fn outer_with_nested_function() {
    if true { fn inner() { if true {} } }
}

fn no_control() {}
"#,
    )
    .expect("write Rust source");

    let report = analyze(&source);
    for (name, expected) in [
        ("sumOfPrimes", 7),
        ("getWords", 1),
        ("nested_macro_injection", 3),
        ("closure_inside_if", 4),
        ("async_block", 1),
        ("if_inside_if_condition", 3),
        ("boolean_runs", 8),
        ("else_if_nesting", 7),
        ("match_guard", 3),
        ("let_else_condition_nested", 3),
        ("let_chain", 3),
        ("labeled_break", 4),
        ("no_control", 0),
    ] {
        assert_eq!(
            function(&report, name)["cognitive_complexity"],
            expected,
            "unexpected cognitive complexity for {name}"
        );
    }
    assert_eq!(
        function(&report, "outer_with_nested_function")["cognitive_complexity"],
        1
    );
    assert_eq!(function(&report, "inner")["cognitive_complexity"], 3);
}
