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

fn analyze_source(root: &Path, name: &str, source: &str) -> Value {
    let path = root.join(name);
    write(&path, source);
    report(&run("file", &path, None, Some("cc,cogc,halstead")))
}

fn write(path: &Path, contents: impl AsRef<[u8]>) {
    std::fs::create_dir_all(path.parent().expect("fixture parent")).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn file<'a>(report: &'a Value, suffix: &str) -> &'a Value {
    report["files"]
        .as_array()
        .expect("file reports")
        .iter()
        .find(|file| {
            file["path"]
                .as_str()
                .is_some_and(|path| path.ends_with(suffix))
        })
        .unwrap_or_else(|| panic!("missing file ending in {suffix}"))
}

fn function<'a>(file: &'a Value, name: &str) -> &'a Value {
    file["functions"]
        .as_array()
        .expect("function reports")
        .iter()
        .find(|function| function["name"] == name)
        .unwrap_or_else(|| panic!("missing function {name}"))
}

fn change<'a>(report: &'a Value, suffix: &str) -> &'a Value {
    report["patch"]["files"]
        .as_array()
        .expect("patch file reports")
        .iter()
        .find(|change| {
            change["path"]
                .as_str()
                .is_some_and(|path| path.ends_with(suffix))
        })
        .unwrap_or_else(|| panic!("missing patch for {suffix}"))
}

fn recursive(function: &Value) -> bool {
    function["cognitive_contributions"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["kind"] == "recursion"))
}

fn has_resolution(file: &Value, name: &str, status: &str) -> bool {
    file["resolution"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["path"]
                .as_array()
                .is_some_and(|path| path.iter().any(|part| part == name))
                && item["resolution"]["status"] == status
        })
    })
}

fn resolution_at<'a>(file: &'a Value, source: &str, needle: &str, occurrence: usize) -> &'a Value {
    let start = source
        .match_indices(needle)
        .nth(occurrence)
        .unwrap_or_else(|| panic!("missing occurrence {occurrence} of {needle:?}"))
        .0;
    let reference_start = if needle.starts_with('{') {
        start + 1
    } else {
        start
    };
    file["resolution"]
        .as_array()
        .expect("resolution list")
        .iter()
        .find(|item| item["start_byte"] == reference_start)
        .unwrap_or_else(|| panic!("missing reference at byte {reference_start} for {needle:?}"))
}

#[path = "web_languages/html.rs"]
mod html;
#[path = "web_languages/javascript.rs"]
mod javascript;
#[path = "web_languages/patch.rs"]
mod patch;
#[path = "web_languages/resolution.rs"]
mod resolution;
#[path = "web_languages/svelte.rs"]
mod svelte;
#[path = "web_languages/svelte_scope.rs"]
mod svelte_scope;
#[path = "web_languages/templates.rs"]
mod templates;
