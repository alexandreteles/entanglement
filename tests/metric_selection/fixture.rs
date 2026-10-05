use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

pub(super) struct Project {
    _temp: tempfile::TempDir,
    pub(super) root: PathBuf,
    pub(super) lib: PathBuf,
    pub(super) change: PathBuf,
}

pub(super) struct Mode<'a> {
    pub(super) command: &'static str,
    pub(super) path: &'a Path,
    pub(super) diff: Option<&'a Path>,
}

pub(super) fn project() -> Project {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let root = temp.path().join("crate");
    let source = root.join("src");
    std::fs::create_dir_all(&source).expect("create source directory");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = 'metric-selection-fixture'\nversion = '0.1.0'\n",
    )
    .expect("write manifest");
    let lib = source.join("lib.rs");
    std::fs::write(
        &lib,
        "pub mod peer;\npub fn left(flag: bool) -> i32 {\n    if flag { peer::right(false) } else { 0 }\n}\n",
    )
    .expect("write crate root");
    std::fs::write(
        source.join("peer.rs"),
        "pub fn right(flag: bool) -> i32 {\n    if flag { crate::left(false) } else { 1 }\n}\n",
    )
    .expect("write peer module");
    std::fs::write(
        source.join("old.rs"),
        "pub fn removed(flag: bool) -> bool { if flag { true } else { false } }\n",
    )
    .expect("write deletion fixture");
    let change = temp.path().join("change.diff");
    std::fs::write(
        &change,
        "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,4 +1,4 @@\n pub mod peer;\n pub fn left(flag: bool) -> i32 {\n-    if flag { peer::right(false) } else { 0 }\n+    if flag { 1 } else if !flag { 0 } else { 0 }\n }\n",
    )
    .expect("write change patch");
    Project {
        _temp: temp,
        root,
        lib,
        change,
    }
}

pub(super) fn modes(project: &Project) -> [Mode<'_>; 4] {
    [
        Mode {
            command: "file",
            path: &project.lib,
            diff: None,
        },
        Mode {
            command: "repo",
            path: &project.root,
            diff: None,
        },
        Mode {
            command: "patch",
            path: &project.lib,
            diff: Some(&project.change),
        },
        Mode {
            command: "candidate",
            path: &project.root,
            diff: Some(&project.change),
        },
    ]
}

pub(super) fn run(mode: &Mode<'_>, format: &str, metrics: &[&str], before_command: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_entanglement"));
    if before_command {
        for metric in metrics {
            command.arg("--metrics").arg(metric);
        }
    }
    command.args(["--format", format, mode.command]);
    if !before_command {
        for metric in metrics {
            command.arg("--metrics").arg(metric);
        }
    }
    if let Some(diff) = mode.diff {
        command.arg("--diff").arg(diff);
    }
    command.arg(mode.path).output().expect("run entanglement")
}

pub(super) fn json(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "entanglement failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("parse JSON report")
}
