use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn fixture(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/baseline")
        .join(relative)
}

fn run(arguments: &[&str], path: &Path, diff: Option<&Path>) -> Value {
    let mut command = Command::new(env!("CARGO_BIN_EXE_entanglement"));
    command.args(["--format", "json"]);
    command.args(arguments);
    command.arg(path);
    if let Some(diff) = diff {
        command.arg("--diff").arg(diff);
    }

    let output = command.output().expect("run entanglement");
    assert_success(&output);
    serde_json::from_slice(&output.stdout).expect("parse JSON report")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "entanglement failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn file<'a>(report: &'a Value, suffix: &str) -> &'a Value {
    report["files"]
        .as_array()
        .expect("files array")
        .iter()
        .find(|file| {
            file["path"]
                .as_str()
                .is_some_and(|path| path.ends_with(suffix))
        })
        .unwrap_or_else(|| panic!("report has no file ending in {suffix}"))
}

fn function<'a>(file: &'a Value, name: &str) -> &'a Value {
    file["functions"]
        .as_array()
        .expect("functions array")
        .iter()
        .find(|function| function["name"] == name)
        .unwrap_or_else(|| panic!("file has no function named {name}"))
}

fn resolution(reference: &Value) -> &Value {
    &reference["resolution"]
}

fn references_with_path<'a>(file: &'a Value, path: &[&str]) -> Vec<&'a Value> {
    file["resolution"]
        .as_array()
        .expect("resolution array")
        .iter()
        .filter(|reference| {
            reference["path"].as_array().is_some_and(|parts| {
                parts.len() == path.len()
                    && parts
                        .iter()
                        .zip(path)
                        .all(|(part, expected)| part.as_str() == Some(*expected))
            })
        })
        .collect()
}

fn patch_file(report: &Value) -> &Value {
    report["patch"]["files"]
        .as_array()
        .expect("patch files array")
        .iter()
        .find(|file| {
            file["path"]
                .as_str()
                .is_some_and(|path| path.ends_with("/src/client.rs"))
        })
        .expect("client patch report")
}

fn assert_patch_metrics(report: &Value) {
    let changed = patch_file(report);
    let delta = changed["functions"]
        .as_array()
        .expect("function deltas")
        .iter()
        .find(|function| function["name"] == "calls")
        .expect("calls function delta");

    assert_eq!(delta["nloc"]["before"], 9);
    assert_eq!(delta["nloc"]["after"], 9);
    assert_eq!(delta["nloc"]["delta"], 0);
    assert_eq!(delta["cyclomatic_complexity"]["before"], 2);
    assert_eq!(delta["cyclomatic_complexity"]["after"], 3);
    assert_eq!(delta["cyclomatic_complexity"]["delta"], 1);
    assert!((delta["cyclomatic_density"]["before"].as_f64().unwrap() - 2.0 / 9.0).abs() < 1e-12);
    assert!((delta["cyclomatic_density"]["after"].as_f64().unwrap() - 1.0 / 3.0).abs() < 1e-12);
    assert!((delta["cyclomatic_density"]["delta"].as_f64().unwrap() - 1.0 / 9.0).abs() < 1e-12);

    let added = delta["added_contributions"]
        .as_array()
        .expect("added contributions");
    assert!(
        added
            .iter()
            .any(|item| { item["kind"] == "logical_condition" && item["value"] == 1 })
    );
}

#[test]
fn file_mode_preserves_rust_metrics_and_injection_ranges() {
    let report = run(&["file"], &fixture("file.rs"), None);
    assert_eq!(report["files"].as_array().unwrap().len(), 1);

    let source = file(&report, "/file.rs");
    assert_eq!(source["language"], "rust");
    assert_eq!(source["nloc"], 18);

    let analyze = function(source, "analyze");
    assert_eq!(analyze["nloc"], 15);
    assert_eq!(analyze["cyclomatic_complexity"], 10);
    assert!((analyze["cyclomatic_density"].as_f64().unwrap() - 2.0 / 3.0).abs() < 1e-12);

    let contributions = analyze["contributions"]
        .as_array()
        .expect("complexity contributions");
    assert_eq!(
        contributions
            .iter()
            .filter(|item| item["kind"] == "baseline")
            .count(),
        1
    );
    assert_eq!(
        contributions
            .iter()
            .filter(|item| item["kind"] == "condition")
            .count(),
        4
    );
    assert_eq!(
        contributions
            .iter()
            .filter(|item| item["kind"] == "logical_condition")
            .count(),
        2
    );
    assert!(
        contributions
            .iter()
            .any(|item| item["kind"] == "multiway" && item["value"] == 3)
    );

    let embedded = function(source, "embedded");
    assert_eq!(embedded["cyclomatic_complexity"], 1);
    assert_eq!(source["injections"].as_array().unwrap().len(), 1);
    assert_eq!(source["injections"][0]["language"], "html");
    assert_eq!(source["injections"][0]["analyzed"], false);
}

#[test]
fn repository_mode_preserves_import_shadowing_and_shared_root_resolution() {
    let report = run(&["repo"], &fixture("repo"), None);
    assert_eq!(report["files"].as_array().unwrap().len(), 5);

    let client = file(&report, "/src/client.rs");
    let dispatch = references_with_path(client, &["dispatch"]);
    assert_eq!(dispatch.len(), 2);
    assert_eq!(resolution(dispatch[0])["status"], "exact");
    assert_eq!(resolution(dispatch[0])["symbols"]["name"], "serve");
    assert_eq!(resolution(dispatch[1])["status"], "unresolved");

    let target = references_with_path(client, &["crate", "api", "target"]);
    assert_eq!(target.len(), 1);
    assert_eq!(resolution(target[0])["status"], "exact");
    assert_eq!(resolution(target[0])["symbols"]["name"], "target");

    let shared = file(&report, "/src/shared.rs");
    let local_ping = references_with_path(shared, &["ping"]);
    assert_eq!(local_ping.len(), 1);
    assert_eq!(resolution(local_ping[0])["status"], "exact");

    // shared.rs is reachable from both lib.rs and main.rs; its crate-qualified
    // reference must stay unresolved instead of being assigned to one root.
    let cross_root = references_with_path(shared, &["crate", "api", "serve"]);
    assert_eq!(cross_root.len(), 1);
    assert_eq!(resolution(cross_root[0])["status"], "unresolved");
}

#[test]
fn patch_and_candidate_modes_preserve_metric_deltas() {
    let diff = fixture("patches/client.diff");
    let client = fixture("repo/src/client.rs");
    let original_source = std::fs::read(&client).expect("read fixture source");
    let patch = run(&["patch"], &client, Some(&diff));
    assert_eq!(
        std::fs::read(&client).expect("read fixture source"),
        original_source
    );
    assert_patch_metrics(&patch);
    let changed = patch_file(&patch);
    assert_eq!(
        function(&changed["before"], "calls")["cyclomatic_complexity"],
        2
    );
    assert_eq!(
        function(&changed["after"], "calls")["cyclomatic_complexity"],
        3
    );

    let candidate = run(&["candidate"], &fixture("repo"), Some(&diff));
    assert_eq!(
        std::fs::read(&client).expect("read fixture source"),
        original_source
    );
    assert_eq!(candidate["files"].as_array().unwrap().len(), 5);
    assert_patch_metrics(&candidate);
    let updated_client = file(&candidate, "/src/client.rs");
    assert_eq!(
        function(updated_client, "calls")["cyclomatic_complexity"],
        3
    );
    let dispatch = references_with_path(updated_client, &["dispatch"]);
    assert_eq!(resolution(dispatch[0])["status"], "exact");
    assert_eq!(resolution(dispatch[1])["status"], "unresolved");
}
