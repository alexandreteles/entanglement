use super::{file, report, resolutions, run, status, write};

fn patch(path: &str, before: &str, after: &str) -> String {
    let diff = diffy::create_patch(before, after).to_string();
    diff.replacen("--- original", &format!("--- a/{path}"), 1)
        .replacen("+++ modified", &format!("+++ b/{path}"), 1)
}

#[test]
fn module_path_edits_use_virtual_manifest_bytes_and_match_fresh_analysis() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("app");
    let before_mod = "module example.com/old\n";
    let after_mod = "module example.com/new\n";
    let before_go = "package app\nimport \"example.com/old/dep\"\nfunc Use() { dep.F() }\n";
    let after_go = before_go.replace("example.com/old", "example.com/new");
    write(&root.join("go.mod"), before_mod);
    write(&root.join("main.go"), before_go);
    write(&root.join("dep/dep.go"), "package dep\nfunc F() {}\n");
    let diff = temp.path().join("change.diff");
    write(
        &diff,
        &(patch("go.mod", before_mod, after_mod) + &patch("main.go", before_go, &after_go)),
    );
    let candidate = report(&run(&["candidate"], &root, Some(&diff)));
    assert_eq!(
        std::fs::read_to_string(root.join("go.mod")).unwrap(),
        before_mod
    );
    assert_eq!(
        status(&resolutions(file(&candidate, "main.go")), "dep.F"),
        ["exact dep.go:F"]
    );
    write(&root.join("go.mod"), after_mod);
    write(&root.join("main.go"), &after_go);
    let fresh = report(&run(&["repo"], &root, None));
    assert_eq!(candidate["files"], fresh["files"]);
}

#[test]
fn creating_a_nested_manifest_removes_false_parent_module_targets() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("app");
    write(&root.join("go.mod"), "module example.com/app\n");
    write(&root.join("sub/sub.go"), "package sub\nfunc F() {}\n");
    write(
        &root.join("main.go"),
        "package app\nimport \"example.com/app/sub\"\nfunc Use() { sub.F() }\n",
    );
    let diff = temp.path().join("change.diff");
    write(
        &diff,
        "--- /dev/null\n+++ b/sub/go.mod\n@@ -0,0 +1 @@\n+module example.com/other\n",
    );
    let candidate = report(&run(&["candidate"], &root, Some(&diff)));
    assert!(!root.join("sub/go.mod").exists());
    assert_eq!(
        status(&resolutions(file(&candidate, "main.go")), "sub.F"),
        ["unresolved"]
    );
    write(&root.join("sub/go.mod"), "module example.com/other\n");
    let fresh = report(&run(&["repo"], &root, None));
    assert_eq!(candidate["files"], fresh["files"]);
}

#[test]
fn deleting_a_nested_manifest_restores_parent_module_resolution() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("app");
    write(&root.join("go.mod"), "module example.com/app\n");
    write(&root.join("sub/go.mod"), "module example.com/other\n");
    write(&root.join("sub/sub.go"), "package sub\nfunc F() {}\n");
    write(
        &root.join("main.go"),
        "package app\nimport \"example.com/app/sub\"\nfunc Use() { sub.F() }\n",
    );
    let diff = temp.path().join("change.diff");
    write(
        &diff,
        "--- a/sub/go.mod\n+++ /dev/null\n@@ -1 +0,0 @@\n-module example.com/other\n",
    );
    let candidate = report(&run(&["candidate"], &root, Some(&diff)));
    assert!(root.join("sub/go.mod").exists());
    assert_eq!(
        status(&resolutions(file(&candidate, "main.go")), "sub.F"),
        ["exact sub.go:F"]
    );
    std::fs::remove_file(root.join("sub/go.mod")).unwrap();
    let fresh = report(&run(&["repo"], &root, None));
    assert_eq!(candidate["files"], fresh["files"]);
}
