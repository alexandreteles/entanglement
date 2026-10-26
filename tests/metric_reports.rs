#[path = "metric_reports/assertions.rs"]
mod assertions;
use assertions::*;

mod support;
use support::{function, only_file, report, run, write};

use serde_json::{Value, json};
use std::fmt::Write;

const BASE: &str = "pub fn target(left: i32, right: i32) -> i32 {\n    left + right\n}\n";
const BRANCH: &str = "pub fn target(left: i32, right: i32) -> i32 {\n    if left > 0 {\n        left * right\n    } else {\n        left - right\n    }\n}\n";
#[test]
fn file_repo_and_human_reports_preserve_metric_formulas_and_rating_bands() {
    let (_temp, root, source) = support::rust_project(&format!(
        "{BASE}pub fn second(flag: bool) {{ if flag {{}} }}\n"
    ));
    let result = report(&run("file", &source, None, None));
    let file = only_file(&result);
    assert_halstead(&file["halstead"]);
    assert_index(&file["maintainability_index"]);
    assert_eq!(
        file["maintainability_index"]["volume"],
        file["halstead"]["volume"]
    );
    assert_eq!(file["maintainability_index"]["nloc"], file["nloc"]);
    for item in file["functions"].as_array().unwrap() {
        assert_halstead(&item["halstead"]);
        let index = &item["maintainability_index"];
        assert_index(index);
        assert_eq!(index["volume"], item["halstead"]["volume"]);
        assert_eq!(index["nloc"], item["nloc"]);
        assert_eq!(
            index["cyclomatic_complexity"],
            item["cyclomatic_complexity"]
        );
    }
    assert_eq!(
        file["maintainability_index"]["cyclomatic_complexity"]
            .as_u64()
            .unwrap(),
        file["functions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["cyclomatic_complexity"].as_u64().unwrap())
            .sum::<u64>()
    );
    let metrics = &function(file, "target")["halstead"];
    for (key, expected) in [
        ("distinct_operators", 10),
        ("distinct_operands", 4),
        ("total_operators", 11),
        ("total_operands", 8),
    ] {
        assert_eq!(metrics[key], expected, "{key}");
    }
    for key in ["operators", "operands"] {
        assert!(!metrics[key].as_array().unwrap().is_empty());
    }
    assert_eq!(
        result["maintainability_index_bands"],
        json!([
            {"rating": "red_low", "minimum_inclusive": 0.0, "maximum": 10.0,
             "maximum_inclusive": false, "color": "red", "meaning": "low maintainability"},
            {"rating": "yellow_moderate", "minimum_inclusive": 10.0, "maximum": 20.0,
             "maximum_inclusive": false, "color": "yellow", "meaning": "moderate maintainability"},
            {"rating": "green_good", "minimum_inclusive": 20.0, "maximum": 100.0,
             "maximum_inclusive": true, "color": "green", "meaning": "good maintainability"},
        ])
    );
    let repo = report(&run("repo", &root, None, None));
    for key in ["halstead", "maintainability_index", "functions"] {
        assert_eq!(only_file(&repo)[key], file[key]);
    }
    support::assert_human(
        "file",
        &source,
        None,
        &[
            "File Halstead",
            "Halstead for target",
            "program level",
            "estimated bugs",
            "0–<10 red",
            "10–<20 yellow",
            "20–100 green",
            "File maintainability index",
            "Maintainability index for target",
        ],
    );
    for text in ["", "// comments only\n"] {
        let (_empty, _, empty_source) = support::rust_project(text);
        let empty = report(&run("file", &empty_source, None, None));
        let index = &only_file(&empty)["maintainability_index"];
        assert_eq!(index["score"], 100.0);
        assert_eq!(index["rating"], "green_good");
        assert_eq!(
            ["volume", "nloc", "cyclomatic_complexity"].map(|key| index[key].as_f64().unwrap()),
            [0.0; 3]
        );
    }
}

#[test]
fn patch_and_candidate_explain_metric_changes_without_modifying_source() {
    let (_temp, root, source) = support::rust_project(BASE);
    let diff = root.join("change.diff");
    write(
        &diff,
        "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 +1,7 @@\n pub fn target(left: i32, right: i32) -> i32 {\n-    left + right\n+    if left > 0 {\n+        left * right\n+    } else {\n+        left - right\n+    }\n }\n",
    );
    for (mode, path) in [("patch", &source), ("candidate", &root)] {
        let result = report(&run(mode, path, Some(&diff), None));
        assert_eq!(result, report(&run(mode, path, Some(&diff), None)));
        let file = &result["patch"]["files"][0];
        let target = function(file, "target");
        for scope in [file, target] {
            let delta = &scope["halstead"];
            assert_halstead_delta(delta);
            assert!(delta["volume"]["delta"].as_f64().unwrap() > 0.0);
            assert!(has_token(&delta["added_tokens"], "operator", ">"));
            assert!(has_token(&delta["removed_tokens"], "operator", "+"));
            assert!(
                delta["added_tokens"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|token| token["line"].is_number() && token["count"].as_u64().unwrap() > 0)
            );
            let mi = &scope["maintainability_index"];
            assert_index_delta(mi);
            let change = mi["score"]["delta"].as_f64().unwrap();
            assert!(change < 0.0);
            let effects = EFFECTS
                .iter()
                .map(|key| mi[key].as_f64().unwrap())
                .sum::<f64>();
            assert!((change - effects).abs() < 1e-9);
        }
        assert_eq!(target["cyclomatic_complexity"]["delta"], 1);
        assert!(!target["added_contributions"].as_array().unwrap().is_empty());
        assert_eq!(std::fs::read_to_string(&source).unwrap(), BASE);
    }
    support::assert_human(
        "candidate",
        &root,
        Some(&diff),
        &[
            "File Halstead changes",
            "Halstead changes",
            "volume",
            "token changes",
            "score point effects: volume",
            "negative score change is a regression",
        ],
    );
}

#[test]
fn comment_and_format_changes_preserve_tokens_while_deletions_have_contributors() {
    let (_temp, root, _source) = support::rust_project(BASE);
    let comment_diff = root.join("comment.diff");
    std::fs::write(
        &comment_diff,
        "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 +1,4 @@\n+// explanation\n pub fn target(left: i32, right: i32) -> i32 {\n-    left + right\n+  left  +  right // trailing\n }\n",
    )
    .expect("write formatting patch");
    let comment_report = report(&run("candidate", &root, Some(&comment_diff), None));
    let comment_delta = &comment_report["patch"]["files"][0]["halstead"];
    assert_halstead_delta(comment_delta);
    for name in INDICATORS {
        assert_eq!(comment_delta[name]["delta"].as_f64().unwrap(), 0.0);
    }
    assert!(comment_delta["added_tokens"].as_array().unwrap().is_empty());
    assert!(
        comment_delta["removed_tokens"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let (_deleted_temp, deleted_root, deleted_source) = support::rust_project(BRANCH);
    let delete_diff = deleted_root.join("delete.diff");
    std::fs::write(
        &delete_diff,
        "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,7 +1,3 @@\n pub fn target(left: i32, right: i32) -> i32 {\n-    if left > 0 {\n-        left * right\n-    } else {\n-        left - right\n-    }\n+    left + right\n }\n",
    )
    .expect("write deletion patch");
    let deleted = report(&run("patch", &deleted_source, Some(&delete_diff), None));
    let deletion = &deleted["patch"]["files"][0]["halstead"]["removed_tokens"];
    assert!(has_token(deletion, "operator", "if"));
    assert!(has_token(deletion, "operator", "-"));
}

#[test]
fn file_level_changes_and_inserted_duplicate_tokens_have_precise_contributors() {
    let (_temp, root, _source) = support::rust_project(BASE);
    let file_diff = root.join("file-only.diff");
    std::fs::write(
        &file_diff,
        "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 +1,4 @@\n+const LIMIT: i32 = 7;\n pub fn target(left: i32, right: i32) -> i32 {\n     left + right\n }\n",
    )
    .expect("write file-level patch");
    let file_report = report(&run("candidate", &root, Some(&file_diff), None));
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
    let (_repeat_temp, repeat_root, _repeat_source) = support::rust_project(repeated);
    let repeat_diff = repeat_root.join("repeat.diff");
    std::fs::write(
        &repeat_diff,
        "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,4 +1,5 @@\n pub fn target(left: i32, right: i32) -> i32 {\n+    let total = left + right;\n     let total = left + right;\n     total\n }\n",
    )
    .expect("write duplicate-token patch");
    let repeated_report = report(&run("candidate", &repeat_root, Some(&repeat_diff), None));
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

#[test]
fn added_deleted_scopes_have_no_fabricated_score_delta() {
    let (_temp, root, _source) =
        support::rust_project("pub fn existing(flag: bool) -> bool {\n    flag\n}\n");
    let add = root.join("add.diff");
    std::fs::write(
        &add,
        "--- /dev/null\n+++ b/src/fresh.rs\n@@ -0,0 +1,3 @@\n+pub fn fresh(flag: bool) -> bool {\n+    flag\n+}\n",
    )
    .expect("write new-file patch");
    let added = report(&run("candidate", &root, Some(&add), None));
    let added_file = added["patch"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"].as_str().unwrap().contains("fresh.rs"))
        .expect("new file delta");
    for scope in [added_file, &added_file["functions"][0]] {
        let delta = &scope["maintainability_index"];
        assert_index_delta(delta);
        assert!(delta["before"].is_null());
        assert!(delta["after"].is_object());
    }

    let delete = root.join("delete.diff");
    std::fs::write(
        &delete,
        "--- a/src/lib.rs\n+++ /dev/null\n@@ -1,3 +0,0 @@\n-pub fn existing(flag: bool) -> bool {\n-    flag\n-}\n",
    )
    .expect("write deletion patch");
    let deleted = report(&run("candidate", &root, Some(&delete), None));
    let deleted_file = &deleted["patch"]["files"][0];
    for scope in [deleted_file, &deleted_file["functions"][0]] {
        let delta = &scope["maintainability_index"];
        assert_index_delta(delta);
        assert!(delta["before"].is_object());
        assert!(delta["after"].is_null());
    }
}

#[test]
fn candidate_reports_raw_rating_transition_across_a_boundary() {
    let before = "pub fn target(flag: bool) -> bool {\n    flag\n}\n";
    let (_temp, root, _source) = support::rust_project(before);
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

    let candidate = report(&run("candidate", &root, Some(&diff), None));
    let mi = &candidate["patch"]["files"][0]["maintainability_index"];
    assert_eq!(mi["before"]["rating"], "green_good");
    assert_eq!(mi["after"]["rating"], "yellow_moderate");
    assert!(mi["score"]["delta"].as_f64().unwrap() < 0.0);
}
