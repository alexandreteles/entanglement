use std::fmt::Write as FmtWrite;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn fixture(source_text: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let root = temp.path().join("crate");
    let source_dir = root.join("src");
    std::fs::create_dir_all(&source_dir).expect("create source directory");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = 'maintainability-fixture'\nversion = '0.1.0'\n",
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

fn function<'a>(file: &'a Value, name: &str) -> &'a Value {
    file["functions"]
        .as_array()
        .expect("function reports")
        .iter()
        .find(|function| function["name"] == name)
        .unwrap_or_else(|| panic!("missing function {name}"))
}

fn expected_score(volume: f64, cc: usize, nloc: usize) -> f64 {
    let score =
        (171.0 - 5.2 * volume.max(1.0).ln() - 0.23 * cc as f64 - 16.2 * (nloc.max(1) as f64).ln())
            * 100.0
            / 171.0;
    score.clamp(0.0, 100.0)
}

fn assert_index(index: &Value) {
    for key in ["score", "rating", "volume", "cyclomatic_complexity", "nloc"] {
        assert!(
            index.get(key).is_some(),
            "missing Maintainability Index field {key}"
        );
    }
    assert!(index.get("cognitive_complexity").is_none());
    let expected = expected_score(
        index["volume"].as_f64().unwrap(),
        index["cyclomatic_complexity"].as_u64().unwrap() as usize,
        index["nloc"].as_u64().unwrap() as usize,
    );
    assert!((index["score"].as_f64().unwrap() - expected).abs() < 1e-10);
    let score = index["score"].as_f64().unwrap();
    let rating = if score < 10.0 {
        "red_low"
    } else if score < 20.0 {
        "yellow_moderate"
    } else {
        "green_good"
    };
    assert_eq!(index["rating"], rating);
}

fn assert_index_delta(delta: &Value) {
    for key in ["before", "after", "score"] {
        assert!(delta.get(key).is_some(), "missing MI delta field {key}");
    }
    if delta["before"].is_object() && delta["after"].is_object() {
        assert!(delta["score"].is_object());
        for key in [
            "volume_effect",
            "cyclomatic_effect",
            "nloc_effect",
            "clamp_adjustment",
        ] {
            assert!(delta[key].is_number(), "missing score effect {key}");
        }
    } else {
        assert!(delta["score"].is_null());
        for key in [
            "volume_effect",
            "cyclomatic_effect",
            "nloc_effect",
            "clamp_adjustment",
        ] {
            assert!(delta[key].is_null(), "absent side has a fabricated {key}");
        }
    }
}

#[test]
fn file_repo_json_and_human_show_inputs_ratings_and_all_bands() {
    let (_temp, root, source) = fixture(
        "pub fn first(flag: bool) -> bool {\n    if flag { true } else { false }\n}\npub fn second() {}\n",
    );
    let file_report = report(&run(&["file"], &source, None));
    let file = only_file(&file_report);
    let file_index = &file["maintainability_index"];
    assert_index(file_index);
    let functions = file["functions"].as_array().unwrap();
    assert_eq!(
        file_index["cyclomatic_complexity"].as_u64().unwrap(),
        functions
            .iter()
            .map(|function| function["cyclomatic_complexity"].as_u64().unwrap())
            .sum::<u64>()
    );
    assert_index(&function(file, "first")["maintainability_index"]);
    assert_index(&function(file, "second")["maintainability_index"]);

    let bands = file_report["maintainability_index_bands"]
        .as_array()
        .unwrap();
    assert_eq!(bands.len(), 3);
    assert_eq!(bands[0]["minimum_inclusive"], 0.0);
    assert_eq!(bands[0]["maximum"], 10.0);
    assert_eq!(bands[0]["maximum_inclusive"], false);
    assert_eq!(bands[0]["color"], "red");
    assert_eq!(bands[0]["rating"], "red_low");
    assert_eq!(bands[1]["minimum_inclusive"], 10.0);
    assert_eq!(bands[1]["maximum"], 20.0);
    assert_eq!(bands[1]["maximum_inclusive"], false);
    assert_eq!(bands[1]["color"], "yellow");
    assert_eq!(bands[1]["rating"], "yellow_moderate");
    assert_eq!(bands[2]["minimum_inclusive"], 20.0);
    assert_eq!(bands[2]["maximum"], 100.0);
    assert_eq!(bands[2]["maximum_inclusive"], true);
    assert_eq!(bands[2]["color"], "green");
    assert_eq!(bands[2]["rating"], "green_good");

    let repo = report(&run(&["repo"], &root, None));
    assert_eq!(
        only_file(&repo)["maintainability_index"],
        file["maintainability_index"]
    );
    let human = Command::new(env!("CARGO_BIN_EXE_entanglement"))
        .args(["--format", "human", "file"])
        .arg(&source)
        .output()
        .expect("run human report");
    assert!(human.status.success());
    let human = String::from_utf8(human.stdout).expect("human report is UTF-8");
    for text in [
        "0–<10 red",
        "10–<20 yellow",
        "20–100 green",
        "File maintainability index",
        "Maintainability index for first",
    ] {
        assert!(human.contains(text), "human report missing {text}");
    }

    let (_empty_temp, _empty_root, empty_source) = fixture("// comments only\n");
    let empty = report(&run(&["file"], &empty_source, None));
    assert_eq!(only_file(&empty)["maintainability_index"]["score"], 100.0);
    assert_eq!(
        only_file(&empty)["maintainability_index"]["rating"],
        "green_good"
    );
}

#[test]
fn patch_and_candidate_report_mi_regression_and_explain_input_effects() {
    let before = "pub fn target(flag: bool) -> bool {\n    flag\n}\n";
    let (_temp, root, source) = fixture(before);
    let diff = root.join("change.diff");
    std::fs::write(
        &diff,
        "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 +1,5 @@\n pub fn target(flag: bool) -> bool {\n-    flag\n+    if flag {\n+        true\n+    } else { false }\n }\n",
    )
    .expect("write complexity patch");

    let patch = report(&run(&["patch"], &source, Some(&diff)));
    let candidate = report(&run(&["candidate"], &root, Some(&diff)));
    assert_eq!(candidate, report(&run(&["candidate"], &root, Some(&diff))));
    for result in [&patch, &candidate] {
        let file_delta = &result["patch"]["files"][0]["maintainability_index"];
        assert_index_delta(file_delta);
        assert!(file_delta["score"]["delta"].as_f64().unwrap() < 0.0);
        let change = file_delta["score"]["delta"].as_f64().unwrap();
        let effects = ["volume_effect", "cyclomatic_effect", "nloc_effect"]
            .into_iter()
            .map(|key| file_delta[key].as_f64().unwrap())
            .sum::<f64>()
            + file_delta["clamp_adjustment"].as_f64().unwrap();
        assert!((change - effects).abs() < 1e-9);

        let function_delta = result["patch"]["files"][0]["functions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|function| function["name"] == "target")
            .expect("target function delta");
        let mi = &function_delta["maintainability_index"];
        assert_index_delta(mi);
        assert!(mi["score"]["delta"].as_f64().unwrap() < 0.0);
        assert_eq!(function_delta["cyclomatic_complexity"]["delta"], 1);
        assert!(
            !function_delta["added_contributions"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            !result["patch"]["files"][0]["halstead"]["added_tokens"]
                .as_array()
                .unwrap()
                .is_empty()
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
    assert!(human.contains("score point effects: volume"));
    assert!(human.contains("negative score change is a regression"));
}

#[test]
fn added_deleted_scopes_have_no_fabricated_score_delta() {
    let (_temp, root, _source) = fixture("pub fn existing(flag: bool) -> bool {\n    flag\n}\n");
    let add = root.join("add.diff");
    std::fs::write(
        &add,
        "--- /dev/null\n+++ b/src/fresh.rs\n@@ -0,0 +1,3 @@\n+pub fn fresh(flag: bool) -> bool {\n+    flag\n+}\n",
    )
    .expect("write new-file patch");
    let added = report(&run(&["candidate"], &root, Some(&add)));
    let added_file = added["patch"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"].as_str().unwrap().contains("fresh.rs"))
        .expect("new file delta");
    let file_delta = &added_file["maintainability_index"];
    assert_index_delta(file_delta);
    assert!(file_delta["before"].is_null());
    assert!(file_delta["after"].is_object());
    assert!(file_delta["score"].is_null());
    let function_delta = &added_file["functions"][0]["maintainability_index"];
    assert_index_delta(function_delta);
    assert!(function_delta["before"].is_null());
    assert!(function_delta["after"].is_object());
    assert!(function_delta["score"].is_null());

    let delete = root.join("delete.diff");
    std::fs::write(
        &delete,
        "--- a/src/lib.rs\n+++ /dev/null\n@@ -1,3 +0,0 @@\n-pub fn existing(flag: bool) -> bool {\n-    flag\n-}\n",
    )
    .expect("write deletion patch");
    let deleted = report(&run(&["candidate"], &root, Some(&delete)));
    let deleted_file = &deleted["patch"]["files"][0];
    let file_delta = &deleted_file["maintainability_index"];
    assert_index_delta(file_delta);
    assert!(file_delta["before"].is_object());
    assert!(file_delta["after"].is_null());
    assert!(file_delta["score"].is_null());
    let function_delta = &deleted_file["functions"][0]["maintainability_index"];
    assert_index_delta(function_delta);
    assert!(function_delta["before"].is_object());
    assert!(function_delta["after"].is_null());
    assert!(function_delta["score"].is_null());
}

#[test]
fn candidate_reports_raw_rating_transition_across_a_boundary() {
    let before = "pub fn target(flag: bool) -> bool {\n    flag\n}\n";
    let (_temp, root, _source) = fixture(before);
    let diff = root.join("large-change.diff");
    let count = 350;
    let mut patch = format!(
        "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 +1,{} @@\n pub fn target(flag: bool) -> bool {{\n-    flag\n",
        count + 3
    );
    for index in 0..count {
        let _ = writeln!(patch, "+    let value_{index} = flag;");
    }
    let _ = writeln!(patch, "+    value_{}\n }}", count - 1);
    std::fs::write(&diff, patch).expect("write rating-boundary patch");

    let candidate = report(&run(&["candidate"], &root, Some(&diff)));
    let mi = &candidate["patch"]["files"][0]["maintainability_index"];
    assert_eq!(mi["before"]["rating"], "green_good");
    assert_eq!(mi["after"]["rating"], "yellow_moderate");
    assert!(mi["score"]["delta"].as_f64().unwrap() < 0.0);
}
