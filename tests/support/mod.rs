//! Shared fixture operations and report queries for integration tests.
#![allow(dead_code)]

use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

/// Create parent directories and write the complete fixture content.
pub fn write(path: &Path, source: impl AsRef<[u8]>) {
    std::fs::create_dir_all(path.parent().expect("fixture parent")).unwrap();
    std::fs::write(path, source).unwrap();
}

/// Return the function report with the specified name.
pub fn function<'a>(file: &'a Value, name: &str) -> &'a Value {
    file["functions"]
        .as_array()
        .expect("function reports")
        .iter()
        .find(|item| item["name"] == name)
        .unwrap_or_else(|| panic!("missing function {name}"))
}

/// Check command success and read its JSON report.
pub fn report(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "entanglement failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("parse JSON report")
}

/// Return the file report with the specified path suffix.
pub fn file<'a>(report: &'a Value, suffix: &str) -> &'a Value {
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

/// Run a JSON command with optional diff and metric selection.
pub fn run(mode: &str, path: &Path, diff: Option<&Path>, metrics: Option<&str>) -> Output {
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

/// Return the single file report; fail if the file count differs.
pub fn only_file(report: &Value) -> &Value {
    report["files"]
        .as_array()
        .and_then(|files| (files.len() == 1).then(|| &files[0]))
        .expect("one source file report")
}

/// Analyze Rust source in a temporary file and return its JSON report.
pub fn source_report(source: &str) -> Value {
    let directory = tempfile::tempdir().expect("create temporary directory");
    let path = directory.path().join("scope.rs");
    write(&path, source);
    report(&run("file", &path, None, None))
}
