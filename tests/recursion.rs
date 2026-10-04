use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn run_repo(path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_entanglement"))
        .args(["--format", "json", "repo"])
        .arg(path)
        .output()
        .expect("run entanglement")
}

fn run_candidate(path: &Path, diff: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_entanglement"))
        .args(["--format", "json", "candidate", "--diff"])
        .arg(diff)
        .arg(path)
        .output()
        .expect("run candidate analysis")
}

fn write_crate(root: &Path, source: &str) {
    std::fs::create_dir_all(root.join("src")).expect("create source directory");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = 'recursion-fixture'\nversion = '0.1.0'\n",
    )
    .expect("write manifest");
    std::fs::write(root.join("src/lib.rs"), source).expect("write crate root");
}

fn report(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "entanglement failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("parse JSON report")
}

fn function<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["files"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|file| file["functions"].as_array().unwrap())
        .find(|function| function["name"] == name)
        .unwrap_or_else(|| panic!("missing function {name}"))
}

fn is_recursive(function: &Value) -> bool {
    function["cognitive_contributions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|contribution| contribution["kind"] == "recursion")
}

#[test]
fn marks_only_exact_direct_function_cycles() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let root = temp.path().join("crate");
    write_crate(
        &root,
        r#"
mod child;
use child::aliased as via_alias;

pub fn direct() { direct(); }

pub fn qualified_left() { crate::qualified_right(); }
pub fn qualified_right() { qualified_left(); }

pub fn alias_left() { via_alias(); }

pub fn shadow_target() {}
pub fn shadowed() {
    let shadow_target = || {};
    shadow_target();
}

fn choose(function: fn()) -> fn() { function }
pub fn value_target() { value_only(); }
pub fn value_only() {
    let _function_value = value_target;
    choose(value_target)();
}
"#,
    );
    std::fs::write(
        root.join("src/child.rs"),
        "use crate::alias_left as alias_back;\npub fn aliased() { alias_back(); }\n",
    )
    .expect("write child module");

    let report = report(&run_repo(&root));

    for name in [
        "direct",
        "qualified_left",
        "qualified_right",
        "alias_left",
        "aliased",
    ] {
        assert!(
            is_recursive(function(&report, name)),
            "{name} should be recursive"
        );
        assert_eq!(function(&report, name)["cyclomatic_complexity"], 1);
    }
    for name in [
        "shadow_target",
        "shadowed",
        "choose",
        "value_target",
        "value_only",
    ] {
        assert!(
            !is_recursive(function(&report, name)),
            "{name} must stay acyclic"
        );
    }
}

#[test]
fn candidate_cycle_introduction_marks_unchanged_peer() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let root = temp.path().join("crate");
    write_crate(&root, "pub fn left() {}\npub fn right() { left(); }\n");
    let before = report(&run_repo(&root));
    assert!(!is_recursive(function(&before, "left")));
    assert!(!is_recursive(function(&before, "right")));

    let diff = temp.path().join("introduce.diff");
    std::fs::write(
        &diff,
        "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,2 +1,2 @@\n-pub fn left() {}\n+pub fn left() { right(); }\n pub fn right() { left(); }\n",
    )
    .expect("write cycle introduction patch");
    let candidate = report(&run_candidate(&root, &diff));
    assert!(is_recursive(function(&candidate, "left")));
    assert!(is_recursive(function(&candidate, "right")));
}

#[test]
fn candidate_cycle_break_removes_recursion_from_unchanged_peer() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let root = temp.path().join("crate");
    write_crate(
        &root,
        "pub fn left() { right(); }\npub fn right() { left(); }\n",
    );
    let before = report(&run_repo(&root));
    assert!(is_recursive(function(&before, "left")));
    assert!(is_recursive(function(&before, "right")));

    let diff = temp.path().join("break.diff");
    std::fs::write(
        &diff,
        "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,2 +1,2 @@\n-pub fn left() { right(); }\n+pub fn left() {}\n pub fn right() { left(); }\n",
    )
    .expect("write cycle break patch");
    let candidate = report(&run_candidate(&root, &diff));
    assert!(!is_recursive(function(&candidate, "left")));
    assert!(
        !is_recursive(function(&candidate, "right")),
        "the untouched member must lose recursion after its peer changes"
    );
}
