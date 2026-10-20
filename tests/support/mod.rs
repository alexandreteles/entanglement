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

/// Return true if the function has a recursion contribution.
pub fn recursive(function: &Value) -> bool {
    function["cognitive_contributions"]
        .as_array()
        .expect("cognitive contributions")
        .iter()
        .any(|item| item["kind"] == "recursion")
}

/// Create a Rust package with the source in `src/lib.rs`.
/// Return the temporary directory, package root, and source path.
pub fn rust_project(source: &str) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let root = temp.path().join("crate");
    write(
        &root.join("Cargo.toml"),
        "[package]\nname = 'fixture'\nversion = '0.1.0'\n",
    );
    let path = root.join("src/lib.rs");
    write(&path, source);
    (temp, root, path)
}

/// Create a unified patch for an existing, added, or deleted file.
/// An absent side uses `/dev/null`; an empty file uses an empty string.
pub fn patch(path: &str, before: Option<&str>, after: Option<&str>) -> String {
    let from = before.map_or_else(|| "/dev/null".to_owned(), |_| format!("a/{path}"));
    let to = after.map_or_else(|| "/dev/null".to_owned(), |_| format!("b/{path}"));
    diffy::create_patch(before.unwrap_or_default(), after.unwrap_or_default())
        .to_string()
        .replacen("--- original", &format!("--- {from}"), 1)
        .replacen("+++ modified", &format!("+++ {to}"), 1)
}

/// Check the command status and all required human report labels.
pub fn assert_human(mode: &str, path: &Path, diff: Option<&Path>, labels: &[&str]) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_entanglement"));
    command.args(["--format", "human", mode]).arg(path);
    if let Some(diff) = diff {
        command.arg("--diff").arg(diff);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    for label in labels {
        assert!(text.contains(label), "missing human report label {label}");
    }
}
