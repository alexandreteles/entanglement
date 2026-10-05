use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn fixture(source_text: &str) -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let root = temp.path().join("crate");
    let source_dir = root.join("src");
    std::fs::create_dir_all(&source_dir).expect("create source directory");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = 'function-range-fixture'\nversion = '0.1.0'\n",
    )
    .expect("write manifest");
    std::fs::write(source_dir.join("lib.rs"), source_text).expect("write source");
    (temp, root)
}

fn run(root: &Path, diff: &Path, metrics: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_entanglement"));
    command.args(["--format", "json"]);
    if let Some(metrics) = metrics {
        command.args(["--metrics", metrics]);
    }
    command
        .args(["candidate"])
        .arg(root)
        .arg("--diff")
        .arg(diff);
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

fn function<'a>(functions: &'a Value, name: &str) -> &'a Value {
    functions
        .as_array()
        .expect("function reports")
        .iter()
        .find(|function| function["name"] == name)
        .unwrap_or_else(|| panic!("missing function {name}"))
}

fn range(function: &Value) -> Value {
    serde_json::json!({
        "start_byte": function["start_byte"],
        "end_byte": function["end_byte"],
    })
}

#[test]
fn function_deltas_include_matched_before_and_after_ranges() {
    let before = "pub fn alpha() {}\npub fn beta() {}\npub fn gone() {}\n";
    let (_temp, root) = fixture(before);
    let diff = root.join("change.diff");
    std::fs::write(
        &diff,
        concat!(
            "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 +1,3 @@\n",
            "+pub fn added() {}\n",
            " pub fn alpha() {}\n",
            " pub fn beta() {}\n",
            "-pub fn gone() {}\n",
        ),
    )
    .expect("write function patch");

    let result = report(&run(&root, &diff, None));
    let file_delta = &result["patch"]["files"][0];
    let old_functions = &file_delta["before"]["functions"];
    let new_functions = &file_delta["after"]["functions"];
    let deltas = &file_delta["functions"];

    for name in ["alpha", "beta"] {
        let delta = function(deltas, name);
        assert_eq!(delta["before_range"], range(function(old_functions, name)));
        assert_eq!(delta["after_range"], range(function(new_functions, name)));
    }

    let added = function(deltas, "added");
    assert!(added.get("before_range").is_some());
    assert!(added["before_range"].is_null());
    assert_eq!(
        added["after_range"],
        range(function(new_functions, "added"))
    );

    let gone = function(deltas, "gone");
    assert_eq!(gone["before_range"], range(function(old_functions, "gone")));
    assert!(gone.get("after_range").is_some());
    assert!(gone["after_range"].is_null());

    let filtered = report(&run(&root, &diff, Some("cc")));
    let filtered_delta = function(&filtered["patch"]["files"][0]["functions"], "beta");
    assert_eq!(
        filtered_delta["before_range"],
        function(deltas, "beta")["before_range"]
    );
    assert_eq!(
        filtered_delta["after_range"],
        function(deltas, "beta")["after_range"]
    );
}
