use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn run(root: &Path, mode: &str, diff: Option<&Path>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_entanglement"));
    command
        .args(["--format", "json", "--metrics", "cogc", mode])
        .arg(root);
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

fn ref_at<'a>(file: &'a Value, source: &str, needle: &str, occurrence: usize) -> &'a Value {
    let start = source
        .match_indices(needle)
        .nth(occurrence)
        .unwrap_or_else(|| panic!("missing {occurrence} occurrence of {needle:?}"))
        .0;
    file["resolution"]
        .as_array()
        .expect("reference analysis")
        .iter()
        .find(|item| item["start_byte"] == start)
        .unwrap_or_else(|| panic!("missing reference at {start} for {needle:?}"))
}

fn exact_target(reference: &Value, suffix: &str, name: &str) {
    assert_eq!(reference["resolution"]["status"], "exact", "{reference:#}");
    assert_eq!(reference["resolution"]["symbols"]["name"], name);
    assert!(
        reference["resolution"]["symbols"]["file"]
            .as_str()
            .is_some_and(|path| path.ends_with(suffix)),
        "{reference:#}"
    );
}

#[path = "kotlin_resolution/advanced.rs"]
mod advanced;
#[path = "kotlin_resolution/build_edges.rs"]
mod build_edges;
#[path = "kotlin_resolution/recursion.rs"]
mod recursion;
#[path = "kotlin_resolution/scopes.rs"]
mod scopes;
#[path = "kotlin_resolution/source_sets.rs"]
mod source_sets;
