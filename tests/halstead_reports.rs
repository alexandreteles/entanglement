use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

const BASE: &str = "pub fn target(left: i32, right: i32) -> i32 {\n    left + right\n}\n";
const BRANCH: &str = "pub fn target(left: i32, right: i32) -> i32 {\n    if left > 0 {\n        left * right\n    } else {\n        left - right\n    }\n}\n";

fn fixture(source_text: &str) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let root = temp.path().join("crate");
    let source_dir = root.join("src");
    std::fs::create_dir_all(&source_dir).expect("create source directory");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = 'halstead-fixture'\nversion = '0.1.0'\n",
    )
    .expect("write manifest");
    let source = source_dir.join("lib.rs");
    std::fs::write(&source, source_text).expect("write source");
    (temp, root, source)
}

fn run(arguments: &[&str], target: &Path, diff: Option<&Path>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_entanglement"));
    command.args(["--format", "json"]).args(arguments);
    if let Some(diff) = diff {
        command.arg("--diff").arg(diff);
    }
    command.arg(target).output().expect("run entanglement")
}

fn report(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "entanglement failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("parse JSON report")
}

fn only_file(report: &Value) -> &Value {
    report["files"]
        .as_array()
        .and_then(|files| (files.len() == 1).then(|| &files[0]))
        .expect("one source file report")
}

fn target_function(file: &Value) -> &Value {
    file["functions"]
        .as_array()
        .expect("function reports")
        .iter()
        .find(|function| function["name"] == "target")
        .expect("target function report")
}

fn assert_indicators(halstead: &Value) {
    for name in [
        "distinct_operators",
        "distinct_operands",
        "total_operators",
        "total_operands",
        "vocabulary",
        "length",
        "estimated_length",
        "volume",
        "difficulty",
        "effort",
        "time",
        "program_level",
        "estimated_bugs",
    ] {
        assert!(
            halstead.get(name).is_some(),
            "missing Halstead indicator {name}"
        );
    }
    for name in [
        "estimated_length",
        "volume",
        "difficulty",
        "effort",
        "time",
        "program_level",
        "estimated_bugs",
    ] {
        assert!(
            halstead[name]
                .as_f64()
                .expect("numeric indicator")
                .is_finite(),
            "non-finite Halstead indicator {name}"
        );
    }
}

fn assert_delta_indicators(delta: &Value) {
    for name in [
        "distinct_operators",
        "distinct_operands",
        "total_operators",
        "total_operands",
        "vocabulary",
        "length",
        "estimated_length",
        "volume",
        "difficulty",
        "effort",
        "time",
        "program_level",
        "estimated_bugs",
    ] {
        assert!(
            delta[name]["before"].is_number(),
            "missing {name} before value"
        );
        assert!(
            delta[name]["after"].is_number(),
            "missing {name} after value"
        );
        assert!(delta[name]["delta"].is_number(), "missing {name} delta");
    }
    assert!(delta["added_tokens"].is_array());
    assert!(delta["removed_tokens"].is_array());
}

#[test]
fn file_and_repo_reports_include_file_and_function_halstead_formulas() {
    let (_temp, root, source) = fixture(BASE);
    let file_report = report(&run(&["file"], &source, None));
    let file = only_file(&file_report);
    let file_metrics = &file["halstead"];
    let function_metrics = &target_function(file)["halstead"];
    assert_indicators(file_metrics);
    assert_indicators(function_metrics);

    assert_eq!(function_metrics["distinct_operators"], 10);
    assert_eq!(function_metrics["distinct_operands"], 4);
    assert_eq!(function_metrics["total_operators"], 11);
    assert_eq!(function_metrics["total_operands"], 8);
    let n1 = function_metrics["distinct_operators"].as_f64().unwrap();
    let n2 = function_metrics["distinct_operands"].as_f64().unwrap();
    let n = function_metrics["vocabulary"].as_f64().unwrap();
    let length = function_metrics["length"].as_f64().unwrap();
    assert_eq!(n, n1 + n2);
    assert_eq!(
        length,
        function_metrics["total_operators"].as_f64().unwrap()
            + function_metrics["total_operands"].as_f64().unwrap()
    );
    let n2_total = function_metrics["total_operands"].as_f64().unwrap();
    let estimated_length = n1 * n1.log2() + n2 * n2.log2();
    let volume = length * n.log2();
    let difficulty = (n1 / 2.0) * (n2_total / n2);
    assert!(
        (function_metrics["estimated_length"].as_f64().unwrap() - estimated_length).abs() < 1e-10
    );
    assert!((function_metrics["volume"].as_f64().unwrap() - volume).abs() < 1e-10);
    assert!((function_metrics["difficulty"].as_f64().unwrap() - difficulty).abs() < 1e-10);
    assert!((function_metrics["effort"].as_f64().unwrap() - difficulty * volume).abs() < 1e-10);
    assert!(
        (function_metrics["time"].as_f64().unwrap() - difficulty * volume / 18.0).abs() < 1e-10
    );
    assert!((function_metrics["program_level"].as_f64().unwrap() - 1.0 / difficulty).abs() < 1e-10);
    assert!((function_metrics["estimated_bugs"].as_f64().unwrap() - volume / 3000.0).abs() < 1e-10);
    assert!(
        function_metrics["operators"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
    assert!(
        function_metrics["operands"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );

    let repo = report(&run(&["repo"], &root, None));
    let repo_file = only_file(&repo);
    assert_eq!(&repo_file["halstead"], file_metrics);
    assert_eq!(repo_file["functions"], file["functions"]);
    let human = Command::new(env!("CARGO_BIN_EXE_entanglement"))
        .args(["--format", "human", "file"])
        .arg(&source)
        .output()
        .expect("run human report");
    assert!(human.status.success());
    let human = String::from_utf8(human.stdout).expect("human report is UTF-8");
    for label in [
        "File Halstead",
        "Halstead for target",
        "program level",
        "estimated bugs",
    ] {
        assert!(human.contains(label), "human report missing {label}");
    }
}

#[test]
fn patch_and_candidate_show_metric_deltas_and_added_removed_contributors() {
    let (_temp, root, source) = fixture(BASE);
    let diff = root.join("change.diff");
    std::fs::write(
        &diff,
        "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 +1,7 @@\n pub fn target(left: i32, right: i32) -> i32 {\n-    left + right\n+    if left > 0 {\n+        left * right\n+    } else {\n+        left - right\n+    }\n }\n",
    )
    .expect("write branch patch");

    let patch = report(&run(&["patch"], &source, Some(&diff)));
    let candidate = report(&run(&["candidate"], &root, Some(&diff)));
    assert_eq!(candidate, report(&run(&["candidate"], &root, Some(&diff))));
    for result in [&patch, &candidate] {
        let file_delta = &result["patch"]["files"][0];
        let delta = &file_delta["halstead"];
        assert_delta_indicators(delta);
        assert!(delta["volume"]["delta"].as_f64().unwrap() > 0.0);
        let function = file_delta["functions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|function| function["name"] == "target")
            .expect("target function delta");
        assert_delta_indicators(&function["halstead"]);
        assert_eq!(function["cyclomatic_complexity"]["delta"], 1);
        assert!(has_token(&delta["added_tokens"], "operator", ">"));
        assert!(has_token(&delta["removed_tokens"], "operator", "+"));
        assert!(
            delta["added_tokens"]
                .as_array()
                .unwrap()
                .iter()
                .all(|token| { token["line"].is_number() && token["count"].as_u64().unwrap() > 0 })
        );
    }

    let human = Command::new(env!("CARGO_BIN_EXE_entanglement"))
        .args(["--format", "human", "candidate", "--diff"])
        .arg(&diff)
        .arg(&root)
        .output()
        .expect("run human candidate report");
    assert!(human.status.success());
    let human = String::from_utf8(human.stdout).expect("human report is UTF-8");
    for label in [
        "File Halstead changes",
        "Halstead changes",
        "volume",
        "token changes",
    ] {
        assert!(
            human.contains(label),
            "candidate human output missing {label}"
        );
    }
}

#[test]
fn comment_and_format_changes_preserve_tokens_while_deletions_have_contributors() {
    let (_temp, root, _source) = fixture(BASE);
    let comment_diff = root.join("comment.diff");
    std::fs::write(
        &comment_diff,
        "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 +1,4 @@\n+// explanation\n pub fn target(left: i32, right: i32) -> i32 {\n-    left + right\n+  left  +  right // trailing\n }\n",
    )
    .expect("write formatting patch");
    let comment_report = report(&run(&["candidate"], &root, Some(&comment_diff)));
    let comment_delta = &comment_report["patch"]["files"][0]["halstead"];
    assert_delta_indicators(comment_delta);
    for name in [
        "distinct_operators",
        "distinct_operands",
        "total_operators",
        "total_operands",
        "vocabulary",
        "length",
        "estimated_length",
        "volume",
        "difficulty",
        "effort",
        "time",
        "program_level",
        "estimated_bugs",
    ] {
        assert_eq!(comment_delta[name]["delta"].as_f64().unwrap(), 0.0);
    }
    assert!(comment_delta["added_tokens"].as_array().unwrap().is_empty());
    assert!(
        comment_delta["removed_tokens"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let (_deleted_temp, deleted_root, deleted_source) = fixture(BRANCH);
    let delete_diff = deleted_root.join("delete.diff");
    std::fs::write(
        &delete_diff,
        "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,7 +1,3 @@\n pub fn target(left: i32, right: i32) -> i32 {\n-    if left > 0 {\n-        left * right\n-    } else {\n-        left - right\n-    }\n+    left + right\n }\n",
    )
    .expect("write deletion patch");
    let deleted = report(&run(&["patch"], &deleted_source, Some(&delete_diff)));
    let deletion = &deleted["patch"]["files"][0]["halstead"]["removed_tokens"];
    assert!(has_token(deletion, "operator", "if"));
    assert!(has_token(deletion, "operator", "-"));
}

#[test]
fn file_level_changes_and_inserted_duplicate_tokens_have_precise_contributors() {
    let (_temp, root, _source) = fixture(BASE);
    let file_diff = root.join("file-only.diff");
    std::fs::write(
        &file_diff,
        "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 +1,4 @@\n+const LIMIT: i32 = 7;\n pub fn target(left: i32, right: i32) -> i32 {\n     left + right\n }\n",
    )
    .expect("write file-level patch");
    let file_report = report(&run(&["candidate"], &root, Some(&file_diff)));
    let file_delta = &file_report["patch"]["files"][0];
    assert!(has_token(
        &file_delta["halstead"]["added_tokens"],
        "operand",
        "LIMIT"
    ));
    let target_delta = file_delta["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|function| function["name"] == "target")
        .expect("target function delta");
    assert!(
        target_delta["halstead"]["added_tokens"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let repeated = "pub fn target(left: i32, right: i32) -> i32 {\n    let total = left + right;\n    total\n}\n";
    let (_repeat_temp, repeat_root, _repeat_source) = fixture(repeated);
    let repeat_diff = repeat_root.join("repeat.diff");
    std::fs::write(
        &repeat_diff,
        "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,4 +1,5 @@\n pub fn target(left: i32, right: i32) -> i32 {\n+    let total = left + right;\n     let total = left + right;\n     total\n }\n",
    )
    .expect("write duplicate-token patch");
    let repeated_report = report(&run(&["candidate"], &repeat_root, Some(&repeat_diff)));
    let repeated_delta = &repeated_report["patch"]["files"][0]["halstead"];
    assert!(
        repeated_delta["added_tokens"]
            .as_array()
            .unwrap()
            .iter()
            .any(|token| token["kind"] == "operator"
                && token["token"] == "+"
                && token["line"] == 2
                && token["count"] == 1)
    );
}

fn has_token(tokens: &Value, kind: &str, text: &str) -> bool {
    tokens.as_array().is_some_and(|items| {
        items
            .iter()
            .any(|token| token["kind"] == kind && token["token"] == text)
    })
}
