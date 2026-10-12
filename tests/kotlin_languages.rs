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
    std::fs::create_dir_all(path.parent().expect("fixture parent")).unwrap();
    std::fs::write(path, source).unwrap();
}

fn file<'a>(report: &'a Value, suffix: &str) -> &'a Value {
    report["files"]
        .as_array()
        .expect("file reports")
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
        .expect("function reports")
        .iter()
        .find(|item| item["name"] == name)
        .unwrap_or_else(|| panic!("missing function {name}"))
}

fn has_operand(halstead: &Value, spelling: &str) -> bool {
    halstead["operands"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["text"]
                .as_str()
                .is_some_and(|text| text.contains(spelling))
        })
    })
}

fn has_operand_at(halstead: &Value, spelling: &str, start_byte: usize) -> bool {
    halstead["operands"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["text"] == spelling && item["start_byte"].as_u64() == Some(start_byte as u64)
        })
    })
}

fn contribution_count(function: &Value, kind: &str) -> usize {
    function["contributions"]
        .as_array()
        .expect("complexity contributions")
        .iter()
        .filter(|item| item["kind"] == kind)
        .count()
}

fn cognitive_contribution_count(function: &Value, kind: &str) -> usize {
    function["cognitive_contributions"]
        .as_array()
        .expect("cognitive complexity contributions")
        .iter()
        .filter(|item| item["kind"] == kind)
        .count()
}

#[path = "kotlin_languages/commands.rs"]
mod commands;
#[path = "kotlin_languages/native.rs"]
mod native;
