use super::*;

#[test]
fn patch_mode_analyzes_typescript_in_memory() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("source.ts");
    let before = "export function first(flag: boolean) { if (flag) return 1; return 0; }\n";
    write(&path, before);
    let diff = temp.path().join("rename.diff");
    write(
        &diff,
        "--- a/source.ts\n+++ b/source.ts\n@@ -1 +1 @@\n-export function first(flag: boolean) { if (flag) return 1; return 0; }\n+export function second(flag: boolean) { if (flag) return 2; return 0; }\n",
    );
    let patched = report(&run("patch", &path, Some(&diff), Some("cc")));
    let change = &patched["patch"]["files"][0];
    assert!(
        change["before"]["functions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["name"] == "first")
    );
    assert!(
        change["after"]["functions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["name"] == "second")
    );
    assert_eq!(std::fs::read(&path).unwrap(), before.as_bytes());
}

#[test]
fn candidate_file_uses_typescript_project_manifests_and_incremental_matches_fresh() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("web-project");
    write(&root.join("tsconfig.json"), "{\"compilerOptions\":{}}\n");
    let left = root.join("src/left.ts");
    let right = root.join("src/right.ts");
    let original = "import { right } from './right';\nexport function left(flag: boolean) { if (flag) right(false); }\n";
    write(&left, original);
    write(
        &right,
        "import { left } from './left';\nexport function right(flag: boolean) { if (flag) left(false); }\n",
    );
    let before_report = report(&run("repo", &root, None, Some("cogc")));
    assert!(recursive(function(
        file(&before_report, "/left.ts"),
        "left"
    )));
    assert!(recursive(function(
        file(&before_report, "/right.ts"),
        "right"
    )));
    let diff = temp.path().join("break-cycle.diff");
    write(
        &diff,
        "--- a/src/left.ts\n+++ b/src/left.ts\n@@ -1,2 +1,2 @@\n import { right } from './right';\n-export function left(flag: boolean) { if (flag) right(false); }\n+export function left(flag: boolean) { if (flag) return; }\n",
    );
    let candidate = report(&run("candidate", &left, Some(&diff), Some("cogc")));
    assert!(candidate["files"].as_array().unwrap().len() >= 2);
    assert!(!recursive(function(file(&candidate, "/left.ts"), "left")));
    assert!(!recursive(function(file(&candidate, "/right.ts"), "right")));
    assert!(candidate["files"][0].get("halstead").is_none());
    assert!(candidate["files"][0].get("maintainability_index").is_none());
    let alias = report(&run(
        "candidate",
        &left,
        Some(&diff),
        Some("cognitive-complexity"),
    ));
    assert_eq!(alias["files"], candidate["files"]);

    let changed = "import { right } from './right';\nexport function left(flag: boolean) { if (flag) return; }\n";
    write(&left, changed);
    let fresh = report(&run("repo", &root, None, Some("cogc")));
    assert_eq!(
        candidate["files"].as_array().unwrap().len(),
        fresh["files"].as_array().unwrap().len()
    );
    assert_eq!(
        function(file(&candidate, "/left.ts"), "left"),
        function(file(&fresh, "/left.ts"), "left")
    );
    assert_eq!(
        function(file(&candidate, "/right.ts"), "right"),
        function(file(&fresh, "/right.ts"), "right")
    );
}

#[test]
fn candidate_tracks_created_deleted_and_renamed_typescript_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    write(&root.join("src/old.ts"), "export function oldName() {}\n");
    write(
        &root.join("src/from.ts"),
        "export function movedName() {}\n",
    );
    let diff = temp.path().join("paths.diff");
    write(
        &diff,
        concat!(
            "--- /dev/null\n+++ b/src/new.ts\n@@ -0,0 +1 @@\n+export function added() {}\n",
            "--- a/src/old.ts\n+++ /dev/null\n@@ -1 +0,0 @@\n-export function oldName() {}\n",
            "--- a/src/from.ts\n+++ b/src/to.ts\n@@ -1 +1 @@\n-export function movedName() {}\n+export function renamed() {}\n",
        ),
    );
    let candidate = report(&run("candidate", &root, Some(&diff), Some("mi,cogc")));
    let files = candidate["patch"]["files"].as_array().unwrap();
    assert_eq!(files.len(), 3);
    assert!(change(&candidate, "/new.ts")["before"].is_null());
    assert!(change(&candidate, "/new.ts")["after"].is_object());
    assert!(change(&candidate, "/old.ts")["before"].is_object());
    assert!(change(&candidate, "/old.ts")["after"].is_null());
    assert!(change(&candidate, "/to.ts")["before"].is_object());
    assert!(change(&candidate, "/to.ts")["after"].is_object());
    assert!(
        file(&candidate, "/new.ts")["functions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["name"] == "added")
    );
    assert!(
        file(&candidate, "/to.ts")["functions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["name"] == "renamed")
    );
    assert_eq!(
        std::fs::read(root.join("src/old.ts")).unwrap(),
        b"export function oldName() {}\n"
    );
    assert_eq!(
        std::fs::read(root.join("src/from.ts")).unwrap(),
        b"export function movedName() {}\n"
    );
}
