use support::write;
mod support;
use support::file;

use std::{path::Path, process::Command};

use serde_json::Value;

fn analyze(root: &Path, diff: Option<&Path>) -> Value {
    let mut command = Command::new(env!("CARGO_BIN_EXE_entanglement"));
    command.args(["--format", "json", "--metrics", "cogc"]);
    command
        .arg(if diff.is_some() { "candidate" } else { "repo" })
        .arg(root);
    if let Some(diff) = diff {
        command.arg("--diff").arg(diff);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn reference<'a>(file: &'a Value, source: &str, needle: &str) -> &'a Value {
    let start = source
        .rfind(needle)
        .unwrap_or_else(|| panic!("Missing source {needle}"));
    &file["resolution"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["start_byte"] == start)
        .unwrap_or_else(|| {
            panic!(
                "Missing reference {needle} at {start}: {}",
                file["resolution"]
            )
        })["resolution"]
}

fn exact(file: &Value, source: &str, needle: &str, suffix: &str, name: &str) {
    let target = reference(file, source, needle);
    assert_eq!(target["status"], "exact", "{needle}: {target}");
    assert_eq!(target["symbols"]["name"], name, "{needle}: {target}");
    assert!(
        target["symbols"]["file"]
            .as_str()
            .is_some_and(|path| path.ends_with(suffix)),
        "{needle}: {target}"
    );
}

fn recursive(file: &Value, name: &str) -> bool {
    let function = file["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|function| function["name"] == name)
        .unwrap_or_else(|| panic!("Missing function {name}: {}", file["functions"]));
    function["cognitive_contributions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|contribution| contribution["kind"] == "recursion")
}

#[path = "php_resolution/autoload.rs"]
mod autoload;
#[path = "php_resolution/dispatch.rs"]
mod dispatch;
#[path = "php_resolution/imports.rs"]
mod imports;
