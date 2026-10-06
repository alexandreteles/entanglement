use std::process::Command;

use serde_json::Value;

fn analyze(source: &str) -> Value {
    let directory = tempfile::tempdir().expect("create temporary directory");
    let path = directory.path().join("scope.rs");
    std::fs::write(&path, source).expect("write Rust source");
    let output = Command::new(env!("CARGO_BIN_EXE_entanglement"))
        .args(["--format", "json", "file"])
        .arg(path)
        .output()
        .expect("run entanglement");
    assert!(
        output.status.success(),
        "analysis failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("parse JSON report")
}

#[test]
fn nested_function_tokens_belong_to_the_smallest_scope() {
    let report = analyze("fn outer() { let outer_value = 1; fn inner() { let inner_value = 2; } }");
    let file = &report["files"][0];
    let functions = file["functions"].as_array().expect("function reports");
    let outer = functions
        .iter()
        .find(|function| function["name"] == "outer")
        .expect("outer function");
    let inner = functions
        .iter()
        .find(|function| function["name"] == "inner")
        .expect("inner function");

    assert!(operand(&outer["halstead"], "outer_value"));
    assert!(operand(&outer["halstead"], "1"));
    assert!(!operand(&outer["halstead"], "inner"));
    assert!(!operand(&outer["halstead"], "inner_value"));
    assert!(operand(&inner["halstead"], "inner_value"));
    assert!(operand(&inner["halstead"], "2"));
    assert!(operand(&file["halstead"], "outer_value"));
    assert!(operand(&file["halstead"], "inner_value"));
}

#[test]
fn a_rust_macro_name_does_not_select_html() {
    let report = analyze("fn page() { let amount = 1; v! { <div>injected</div> } }");
    let file = &report["files"][0];

    assert!(
        !file["injections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| { item["language"] == "html" })
    );
    assert!(operand(&file["halstead"], "amount"));
    let function = &file["functions"][0];
    assert!(operand(&function["halstead"], "amount"));
}

fn operand(halstead: &Value, spelling: &str) -> bool {
    halstead["operands"]
        .as_array()
        .is_some_and(|operands| operands.iter().any(|token| token["text"] == spelling))
}
