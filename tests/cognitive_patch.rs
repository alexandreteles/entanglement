use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let nonce = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "entanglement-cognitive-patch-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temporary directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(arguments: &[&str], path: &Path, diff: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_entanglement"))
        .args(arguments)
        .arg("--diff")
        .arg(diff)
        .arg(path)
        .output()
        .expect("run patch analysis")
}

fn report(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "entanglement failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("parse JSON report")
}

fn function_delta<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["patch"]["files"][0]["functions"]
        .as_array()
        .expect("patch function deltas")
        .iter()
        .find(|function| function["name"] == name)
        .unwrap_or_else(|| panic!("missing patch delta for {name}"))
}

fn assert_cognitive_change(report: &Value) {
    let function = function_delta(report, "target");
    assert_eq!(function["cognitive_complexity"]["before"], 1);
    assert_eq!(function["cognitive_complexity"]["after"], 2);
    assert_eq!(function["cognitive_complexity"]["delta"], 1);

    let added = function["added_cognitive_contributions"]
        .as_array()
        .expect("added cognitive contributions");
    assert_eq!(added.len(), 1);
    assert_eq!(added[0]["kind"], "else");
    assert_eq!(added[0]["value"], 1);
    assert!(
        function["removed_cognitive_contributions"]
            .as_array()
            .expect("removed cognitive contributions")
            .is_empty()
    );

    // Adding `else` changes CogC while preserving the existing CC metric.
    assert_eq!(function["cyclomatic_complexity"]["delta"], 0);
}

#[test]
fn patch_and_candidate_report_cognitive_deltas_and_separate_contributions() {
    let temp = TempDir::new();
    let root = temp.path().join("crate");
    let source_dir = root.join("src");
    std::fs::create_dir_all(&source_dir).expect("create source directory");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = 'cognitive-fixture'\nversion = '0.1.0'\n",
    )
    .expect("write manifest");

    let source = source_dir.join("lib.rs");
    let before = "pub fn target(flag: bool) {\n    if flag {}\n}\n";
    std::fs::write(&source, before).expect("write Rust source");

    let diff = temp.path().join("change.diff");
    std::fs::write(
        &diff,
        "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 +1,4 @@\n pub fn target(flag: bool) {\n     if flag {}\n+    else {}\n }\n",
    )
    .expect("write patch");

    let patch = report(&run(&["--format", "json", "patch"], &source, &diff));
    assert_cognitive_change(&patch);

    let candidate = report(&run(&["--format", "json", "candidate"], &root, &diff));
    assert_cognitive_change(&candidate);
    let after = candidate["files"]
        .as_array()
        .expect("candidate files")
        .iter()
        .flat_map(|file| file["functions"].as_array().into_iter().flatten())
        .find(|function| function["name"] == "target")
        .expect("candidate target function");
    assert_eq!(after["cognitive_complexity"], 2);

    let human = Command::new(env!("CARGO_BIN_EXE_entanglement"))
        .args(["--format", "human", "file"])
        .arg(&source)
        .output()
        .expect("run human report");
    assert!(
        human.status.success(),
        "entanglement failed: {}",
        String::from_utf8_lossy(&human.stderr)
    );
    let human = String::from_utf8(human.stdout).expect("human report is UTF-8");
    assert!(human.lines().any(|line| line.contains("CogC")));
    assert!(human.contains("target"));

    let human_patch = Command::new(env!("CARGO_BIN_EXE_entanglement"))
        .args(["--format", "human", "candidate", "--diff"])
        .arg(&diff)
        .arg(&root)
        .output()
        .expect("run human patch report");
    assert!(
        human_patch.status.success(),
        "entanglement failed: {}",
        String::from_utf8_lossy(&human_patch.stderr)
    );
    let human_patch = String::from_utf8(human_patch.stdout).expect("human patch report is UTF-8");
    assert!(human_patch.contains("Cognitive complexity 1 → 2 (+1)"));
    assert!(human_patch.contains("Cognitive complexity changes"));
}
