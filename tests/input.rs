use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn run(mode: &str, path: &Path, diff: &Path) -> Result<Value, Output> {
    let output = Command::new(env!("CARGO_BIN_EXE_entanglement"))
        .args(["--format", "json", mode])
        .arg(path)
        .arg("--diff")
        .arg(diff)
        .output()
        .expect("run entanglement");
    if output.status.success() {
        Ok(serde_json::from_slice(&output.stdout).expect("parse JSON report"))
    } else {
        Err(output)
    }
}

fn patch_file<'a>(report: &'a Value, suffix: &str) -> &'a Value {
    report["patch"]["files"]
        .as_array()
        .expect("patch files")
        .iter()
        .find(|file| {
            file["path"]
                .as_str()
                .is_some_and(|path| path.ends_with(suffix))
        })
        .unwrap_or_else(|| panic!("patch report has no file ending in {suffix}"))
}

fn write(path: &Path, contents: impl AsRef<[u8]>) {
    std::fs::create_dir_all(path.parent().expect("fixture parent")).unwrap();
    std::fs::write(path, contents).unwrap();
}

#[test]
fn patch_accepts_quoted_unicode_paths_and_eof_markers() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("crate/src/café.rs");
    let diff = temp.path().join("quoted.diff");
    let before = b"pub fn old() {}";
    let after = b"pub fn new() {}";
    write(&source, before);
    write(
        &diff,
        r#"--- "a/src/caf\303\251.rs"
+++ "b/src/caf\303\251.rs"
@@ -1 +1 @@
-pub fn old() {}
\ No newline at end of file
+pub fn new() {}
\ No newline at end of file
"#,
    );

    let report = run("patch", &source, &diff).unwrap();
    let changed = patch_file(&report, "/src/café.rs");
    assert_eq!(changed["before"]["functions"][0]["name"], "old");
    assert_eq!(changed["after"]["functions"][0]["name"], "new");
    assert_eq!(
        changed["after"]["hash"],
        blake3::hash(after).to_hex().to_string()
    );
    assert_eq!(std::fs::read(source).unwrap(), before);
}

#[test]
fn candidate_applies_multiple_and_zero_length_edits_across_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("crate");
    let edit_before = b"pub fn target(flag: bool) {\n    let first = 1;\n    let second = 2;\n}\n";
    let edit_after =
        b"pub fn target(flag: bool) {\n    let first = 123;\n    if flag { let second = 4; }\n}\n";
    let insert_before = b"head\nremove\ntail\n";
    let insert_after = b"head\nadded\ntail\n";
    let headers_before = b"-- old\n++ plus\n";
    let headers_after = b"++ new\n++ plus\n";
    for (name, contents) in [
        ("edit.rs", edit_before.as_slice()),
        ("insert.rs", insert_before.as_slice()),
        ("headers.rs", headers_before.as_slice()),
    ] {
        write(&root.join("src").join(name), contents);
    }

    let diff = temp.path().join("multiple.diff");
    write(
        &diff,
        concat!(
            "--- a/src/edit.rs\n+++ b/src/edit.rs\n",
            "@@ -1,2 +1,2 @@\n pub fn target(flag: bool) {\n",
            "-    let first = 1;\n+    let first = 123;\n",
            "@@ -3 +3 @@\n-    let second = 2;\n",
            "+    if flag { let second = 4; }\n",
            "--- a/src/insert.rs\n+++ b/src/insert.rs\n",
            "@@ -1,0 +2 @@\n+added\n@@ -2 +2,0 @@\n-remove\n",
            "--- a/src/headers.rs\n+++ b/src/headers.rs\n",
            "@@ -1,2 +1,2 @@\n--- old\n+++ new\n ++ plus\n",
        ),
    );

    let report = run("candidate", &root, &diff).unwrap();
    assert_eq!(report["patch"]["files"].as_array().unwrap().len(), 3);
    for (suffix, expected, original) in [
        (
            "/src/edit.rs",
            edit_after.as_slice(),
            edit_before.as_slice(),
        ),
        (
            "/src/insert.rs",
            insert_after.as_slice(),
            insert_before.as_slice(),
        ),
        (
            "/src/headers.rs",
            headers_after.as_slice(),
            headers_before.as_slice(),
        ),
    ] {
        let changed = patch_file(&report, suffix);
        assert_eq!(
            changed["after"]["hash"],
            blake3::hash(expected).to_hex().to_string(),
            "wrong output for {suffix}"
        );
        if suffix == "/src/edit.rs" {
            let target = changed["after"]["functions"]
                .as_array()
                .unwrap()
                .iter()
                .find(|function| function["name"] == "target")
                .expect("target function after multi-edit patch");
            assert_eq!(target["cyclomatic_complexity"], 2);
        }
        assert_eq!(
            std::fs::read(root.join(suffix.trim_start_matches('/'))).unwrap(),
            original
        );
    }
}

#[test]
fn patch_rejects_unsafe_binary_mismatched_and_out_of_order_input() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("crate/src/input.rs");
    let original = b"pub fn first() {}\npub fn second() {}\n";
    write(&source, original);
    let cases = [
        (
            "unsafe path",
            "--- a/../escape.rs\n+++ b/../escape.rs\n@@ -1 +1 @@\n-old\n+new\n",
            "outside the repository",
        ),
        (
            "binary patch",
            "GIT binary patch\nliteral 0\n",
            "binary patches are not supported",
        ),
        (
            "mismatched context",
            "--- a/src/input.rs\n+++ b/src/input.rs\n@@ -1 +1 @@\n-pub fn missing() {}\n+pub fn replacement() {}\n",
            "context does not match",
        ),
        (
            "out-of-order hunks",
            "--- a/src/input.rs\n+++ b/src/input.rs\n@@ -2 +2 @@\n-pub fn second() {}\n+pub fn replacement() {}\n@@ -1 +1 @@\n-pub fn first() {}\n+pub fn replacement() {}\n",
            "overlap or are out of order",
        ),
    ];

    for (name, text, expected_error) in cases {
        let diff = temp.path().join(format!("{name}.diff"));
        write(&diff, text);
        let output = match run("patch", &source, &diff) {
            Ok(_) => panic!("{name} diff unexpectedly succeeded"),
            Err(output) => output,
        };
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected_error),
            "{name} diff had unexpected error: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(std::fs::read(&source).unwrap(), original);
    }
}
