use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, source).unwrap();
}

fn analyze(root: &Path) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_entanglement"))
        .args(["--format", "json", "--metrics", "cogc", "repo"])
        .arg(root)
        .output()
        .expect("run entanglement");
    assert!(
        output.status.success(),
        "entanglement failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("parse report")
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

fn refs(file: &Value) -> &[Value] {
    file["resolution"].as_array().map_or(&[], Vec::as_slice)
}

fn reference<'a>(file: &'a Value, path: &[&str]) -> &'a Value {
    refs(file)
        .iter()
        .find(|item| {
            item["path"].as_array().is_some_and(|parts| {
                parts.len() == path.len()
                    && path
                        .iter()
                        .zip(parts)
                        .all(|(expected, actual)| actual.as_str() == Some(*expected))
            })
        })
        .unwrap_or_else(|| panic!("missing reference path {path:?}"))
}

fn exact_target(file: &Value, path: &[&str], suffix: &str, symbol: &str) {
    let resolution = &reference(file, path)["resolution"];
    assert_eq!(resolution["status"], "exact", "reference {path:?}");
    assert_eq!(resolution["symbols"]["name"], symbol);
    assert!(
        resolution["symbols"]["file"]
            .as_str()
            .is_some_and(|target| target.ends_with(suffix))
    );
}

fn recursive(file: &Value, name: &str) -> bool {
    file["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == name)
        .unwrap_or_else(|| panic!("missing function {name}"))["cognitive_contributions"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["kind"] == "recursion"))
}

#[path = "python_resolution/modules.rs"]
mod modules;
#[path = "python_resolution/scopes.rs"]
mod scopes;
