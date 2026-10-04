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
"#,
    )
    .expect("write Rust source");

    let report = analyze(&source);
    assert_eq!(function(&report, "sumOfPrimes")["cognitive_complexity"], 7);
    assert_eq!(function(&report, "getWords")["cognitive_complexity"], 1);
    assert_eq!(
        function(&report, "nested_macro_injection")["cognitive_complexity"],
        3
    );
    assert_eq!(
        function(&report, "closure_inside_if")["cognitive_complexity"],
        4
    );
    assert_eq!(function(&report, "async_block")["cognitive_complexity"], 1);
    assert_eq!(
        function(&report, "if_inside_if_condition")["cognitive_complexity"],
        3
    );
}
