use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn run(mode: &str, path: &Path, diff: Option<&Path>, metrics: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_entanglement"));
    command.args(["--format", "json"]);
    if let Some(metrics) = metrics {
        command.args(["--metrics", metrics]);
    }
    command.arg(mode).arg(path);
    if let Some(diff) = diff {
        command.arg("--diff").arg(diff);
    }
    command.output().expect("run entanglement")
}

fn report(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "entanglement failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("parse JSON report")
}

fn write(path: &Path, source: impl AsRef<[u8]>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, source).unwrap();
}

fn file<'a>(report: &'a Value, suffix: &str) -> &'a Value {
    report["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| {
            item["path"]
                .as_str()
                .is_some_and(|path| path.ends_with(suffix))
        })
        .unwrap_or_else(|| panic!("missing file {suffix}"))
}

fn function<'a>(file: &'a Value, name: &str) -> &'a Value {
    file["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == name)
        .unwrap_or_else(|| panic!("missing function {name}"))
}

fn recursive(function: &Value) -> bool {
    function["cognitive_contributions"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["kind"] == "recursion"))
}

fn injection_count(file: &Value, language: &str, analyzed: bool) -> usize {
    file["injections"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["language"] == language && item["analyzed"] == analyzed)
        .count()
}

fn named_operand(halstead: &Value, spelling: &str) -> bool {
    halstead["operands"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["text"]
                .as_str()
                .is_some_and(|text| text.contains(spelling))
        })
    })
}

fn operand_count(halstead: &Value, spelling: &str) -> usize {
    halstead["operands"].as_array().map_or(0, |items| {
        items.iter().filter(|item| item["text"] == spelling).count()
    })
}

#[path = "python_languages/commands.rs"]
mod commands;
#[path = "python_languages/embedded.rs"]
mod embedded;
#[path = "python_languages/native.rs"]
mod native;
